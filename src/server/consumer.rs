use std::{
	collections::HashMap, net::SocketAddr, ops::DerefMut, str::FromStr, sync::Arc,
};

use either::Either::Left;
use futures::executor::block_on;
use rpc::{
	inbound::{
		InboundFrame,
		request::{
			InboundRequestFrame, InboundRequestFrameData, config::ConsumerConfig,
		},
	},
	outbound::{
		OutboundFrame,
		request::{OutboundRequestFrame, OutboundRequestFrameData},
		response::{
			OutboundResponseFrame, OutboundResponseFrameData,
			complete_job::CompleteJobResponse, config::ConsumerConfigResponse,
			confirm_job_completion::ConfirmJobCompletionResponse,
			create_job::CreateJobResponse, fail_job::FailJobResponse,
			open_connection::OpenConnectionResponse, sync_job::SyncJobResponse,
		},
	},
};
use rusqlite::OptionalExtension;
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::TcpStream,
	sync::{Mutex, broadcast::error::RecvError},
};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::{
	database::{
		fetch_active_providers, fetch_offers,
		service_connections::{self, Currency},
		service_jobs::{
			consumer::{
				complete::consumer_complete_job, confirm::consumer_confirm_job,
				create::consumer_create_job,
				get_unconfirmed::consumer_get_unconfirmed_job, sync::consumer_sync_job,
			},
			fail::fail_job,
			set_confirmation_error::set_job_confirmation_error,
		},
	},
	p2p::{self, OutboundReqResRequestEnvelope, OutboundStreamRequestResult},
	state::{ConsumerNotification, SharedState},
	util::{
		self, ArcMutex,
		cbor::{CborBufReader, read_cbor, write_cbor},
		to_arc_mutex,
	},
};

mod rpc;

// REFACTOR: Make it transport-agnostic (`stream: T: AsyncRead + AsyncWrite`).
pub async fn handle_connection(
	stream: TcpStream,
	_addr: SocketAddr,
	state: &Arc<SharedState>,
) {
	let (rpc_stream_tx, rpc_stream_rx) =
		tokio::sync::oneshot::channel::<yamux::Stream>();

	let rpc_stream_tx = Arc::new(Mutex::new(Some(rpc_stream_tx)));

	let future_connections = to_arc_mutex(HashMap::<i64, libp2p::Stream>::new());
	let future_connections_clone = future_connections.clone();

	type OpenConnectionHandle = tokio::task::JoinHandle<()>;
	let opened_connections =
		to_arc_mutex(HashMap::<i64, OpenConnectionHandle>::new());
	let opened_connections_clone = opened_connections.clone();

	let _ = util::yamux::YamuxServer::new(stream, None, move |yamux_stream| {
		log::debug!("New yamux stream ({})", yamux_stream.id());

		let rpc_stream_tx = rpc_stream_tx.clone();
		let future_connections = future_connections_clone.clone();
		let opened_connections = opened_connections_clone.clone();

		async move {
			let mut rpc_stream_tx = rpc_stream_tx.lock().await;

			// First stream is the RPC stream.
			if let Some(rpc_stream_tx) = rpc_stream_tx.take() {
				log::debug!("Assigned RPC stream ({})", yamux_stream.id());
				rpc_stream_tx.send(yamux_stream).unwrap();
				return Ok(());
			}

			let mut yamux_compat = yamux_stream.compat();

			let connection_rowid = match yamux_compat.read_i64().await {
				Ok(x) => x,
				Err(e) => {
					log::error!("While reading from Yamux stream: {:?}", e);
					return Ok(());
				}
			};

			if let Some(p2p_stream) =
				future_connections.lock().await.remove(&connection_rowid)
			{
				// Connect yamux & p2p streams.
				let handle = tokio::spawn(async move {
					let mut yamux_buf = [0u8; 8192];
					let mut p2p_buf = [0u8; 8192];
					let mut p2p_compat = p2p_stream.compat();

					loop {
						tokio::select! {
							result = yamux_compat.read(&mut yamux_buf) => {
								match result {
									Ok(size) => {
										if size == 0 {
											log::debug!("Yamux stream EOF");
											break;
										}

										log::debug!("Read {} bytes from Yamux stream", size);

										match p2p_compat.write_all(&yamux_buf[0..size]).await {
											Ok(_) => {
												match p2p_compat.flush().await {
													Ok(_) => {},
													Err(e) => {
														log::error!("Failed to flush the P2P stream: {:?}", e);
													}
												}
											},
											Err(e) => {
												log::error!("Failed to write to P2P stream: {:?}", e);
											}
										}
									},
									Err(e) => {
										log::error!("Failed to read from Yamux stream: {:?}", e);
									},
								}
							}

							result = p2p_compat.read(&mut p2p_buf) => {
								match result {
									Ok(size) => {
										if size == 0 {
											log::debug!("P2P stream EOF");
											break;
										}

										log::debug!("Read {} bytes from P2P stream", size);

										match yamux_compat.write_all(&p2p_buf[0..size]).await {
											Ok(_) => {
												match yamux_compat.flush().await {
													Ok(_) => {},
													Err(e) => {
														log::error!("Failed to flush Yamux stream: {:?}", e);
													}
												}
											},
											Err(e) => {
												log::error!("Failed to write to Yamux stream: {:?}", e);
											}
										}
									},
									Err(e) => {
										log::error!("Failed to read from P2P stream: {:?}", e);
									},
								}
							}
						}
					}
					// match tokio::io::copy_bidirectional(
					// 	&mut yamux_compat,
					// 	&mut p2p_stream.compat(),
					// )
					// .await
					// {
					// 	Ok(_) => {
					// 		log::debug!(
					// 			"Both Yamux & P2P streams shut down ({})",
					// 			connection_rowid
					// 		);
					// 	}

					// 	Err(e) => {
					// 		log::error!("(copy_bidirectional) {:?}", e);
					// 	}
					// }
				});

				opened_connections
					.lock()
					.await
					.insert(connection_rowid, handle);

				log::debug!(
					"✅ Successfully joined Yamux & P2P streams for connection {}",
					connection_rowid
				);
			} else {
				log::warn!(
					"Local connection ID from an incoming \
						Yamux stream not found: {}",
					connection_rowid
				);
			}

			Ok(())
		}
	});

	let rpc_stream = rpc_stream_rx.await.unwrap();
	let mut outbound_requests_counter = 0u32;
	let mut cbor_reader = CborBufReader::new(rpc_stream.compat());
	let (outbound_tx, mut outbound_rx) =
		tokio::sync::mpsc::channel::<OutboundFrame>(16);
	let mut config: Option<ConsumerConfig> = None;

	let consumer_lock = state.consumer.lock().await;
	let mut notifications_rx = consumer_lock.notification_tx.subscribe();
	drop(consumer_lock);

	loop {
		tokio::select! {
			biased;

			result = state.shutdown_token.cancelled() => {
				log::debug!(
					"Breaking RPC loop: {:?}",
					result
				);

				break;
			}

			result = cbor_reader.next_cbor::<InboundFrame>() => {
				match result {
					Ok(Some(InboundFrame::Request(request))) => {
						handle_request(
							state,
							&outbound_tx,
							&mut config,
							request,
							&future_connections
						).await;
					},

					Ok(None) => {
						log::trace!("Reader EOF");
						break;
					}

					Err(e) => {
						log::error!("{}", e);
					}
				}
			}

			frame = outbound_rx.recv() => {
				match frame {
					Some(frame) => {
						log::debug!("➡️ {:?}", frame);
						let _ = write_cbor(cbor_reader.get_mut(), &frame).await;
						let _ = cbor_reader.get_mut().flush().await;
					}

					None => {
						log::trace!("Writer EOF");
						break;
					}
				}
			}

			result = notifications_rx.recv() => {
				match result {
					Ok(event) => {
						if config.is_none() {
							log::warn!("Skipping consumer event because it's not configured yet");
							continue;
						}

						outbound_requests_counter += 1;

						let outbound_frame = OutboundFrame::Request(OutboundRequestFrame {
							id: outbound_requests_counter,
							data: match event {
								ConsumerNotification::OfferRemoved(data) => OutboundRequestFrameData::OfferRemoved(data),
								ConsumerNotification::OfferUpdated(data) => OutboundRequestFrameData::OfferUpdated(data),
								ConsumerNotification::ProviderHeartbeat(data) => OutboundRequestFrameData::ProviderHeartbeat(data),
								ConsumerNotification::ProviderUpdated(data) => OutboundRequestFrameData::ProviderUpdated(data),
							}
						});

						outbound_tx.send(outbound_frame).await.unwrap();
					},

					Err(RecvError::Lagged(e)) => {
						log::warn!("{:?}", e);
					}

					Err(e) => {
						panic!("{:?}", e);
					}
				}
			}
		}
	}

	for handle in opened_connections.lock().await.deref_mut().values_mut() {
		handle.abort();
	}
}

async fn handle_request(
	state: &Arc<SharedState>,
	outbound_tx: &tokio::sync::mpsc::Sender<OutboundFrame>,
	config: &mut Option<ConsumerConfig>,
	request: InboundRequestFrame,
	future_connections: &ArcMutex<HashMap<i64, libp2p::Stream>>,
) {
	log::debug!("⬅️ {:?}", request.data);

	match request.data {
		InboundRequestFrameData::Config(data) => {
			if config.is_some() {
				log::warn!("Already set config, ignoring");
				return;
			}

			*config = Some(data);

			let outbound_frame = {
				let database = state.database.lock().await;
				let providers = fetch_active_providers(&database);

				OutboundFrame::Response(OutboundResponseFrame {
					id: request.id,
					data: OutboundResponseFrameData::Config(ConsumerConfigResponse {
						offers: fetch_offers(
							&database,
							&providers
								.iter()
								.map(|p| p.peer_id.clone())
								.collect::<Vec<_>>(),
						),
						providers,
					}),
				})
			};

			outbound_tx.send(outbound_frame).await.unwrap();
		}

		InboundRequestFrameData::OpenConnection {
			offer_snapshot_id,
			currency,
		} => {
			handle_open_connection_request(
				state,
				outbound_tx,
				request.id,
				offer_snapshot_id,
				currency,
				future_connections.clone(),
			)
			.await;
		}

		InboundRequestFrameData::CreateJob {
			connection_id,
			private_payload,
		} => {
			type ConsumerCreateJobError =
				crate::database::service_jobs::consumer::create::ConsumerCreateJobError;

			let response = match consumer_create_job(
				&mut *state.database.lock().await,
				connection_id,
				private_payload,
			) {
				Ok(job_id) => CreateJobResponse::Ok {
					database_job_id: job_id,
				},

				Err(ConsumerCreateJobError::ConnectionNotFound) => {
					CreateJobResponse::ConnectionNotFound
				}
			};

			let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
				id: request.id,
				data: OutboundResponseFrameData::CreateJob(response),
			});

			let _ = outbound_tx.send(outbound_frame).await;
		}

		InboundRequestFrameData::SyncJob {
			database_job_id,
			provider_job_id,
			private_payload,
			created_at_sync,
		} => {
			type Result =
				crate::database::service_jobs::consumer::sync::ConsumerUpdateJobResult;

			let response = match consumer_sync_job(
				&mut *state.database.lock().await,
				database_job_id,
				provider_job_id,
				private_payload,
				created_at_sync,
			) {
				Result::Ok => SyncJobResponse::Ok,
				Result::InvalidJobId => SyncJobResponse::InvalidJobId,
				Result::AlreadySynced => SyncJobResponse::AlreadySynced,
				Result::ProviderJobIdUniqueness => {
					SyncJobResponse::ProviderJobIdUniqueness
				}
			};

			let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
				id: request.id,
				data: OutboundResponseFrameData::SyncJob(response),
			});

			let _ = outbound_tx.send(outbound_frame).await;
		}

		InboundRequestFrameData::CompleteJob {
			database_job_id,
			completed_at_sync,
			balance_delta,
			public_payload,
			private_payload,
		} => {
			type ConsumerCompleteJobResult = crate::database::service_jobs::consumer::complete::ConsumerCompleteJobResult;

			let p2p_lock = state.p2p.lock().await;
			let public_key = p2p_lock.public_key();
			drop(p2p_lock);

			let response = {
				match public_key {
					Some(public_key) => {
						match consumer_complete_job(
							&mut *state.database.lock().await,
							public_key.to_peer_id().to_base58(),
							database_job_id,
							balance_delta,
							public_payload,
							private_payload,
							completed_at_sync,
							|job_hash| {
								// ADHOC: Functions w/ `rusqlite::Connection` may not be `async`.
								block_on(async {
									state
										.p2p
										.lock()
										.await
										.sign(job_hash)
										.expect("should be running P2P at this point")
										.unwrap()
								})
							},
						)
						.await
						{
							ConsumerCompleteJobResult::Ok => CompleteJobResponse::Ok,

							ConsumerCompleteJobResult::InvalidJobId => {
								CompleteJobResponse::InvalidJobId
							}

							ConsumerCompleteJobResult::ConsumerPeerIdMismatch => {
								CompleteJobResponse::InvalidConsumerPeerId {
									message: "Local peer ID doesn't match the job's".to_string(),
								}
							}

							ConsumerCompleteJobResult::NotSyncedYet => {
								CompleteJobResponse::NotSyncedYet
							}

							ConsumerCompleteJobResult::AlreadyCompleted => {
								CompleteJobResponse::AlreadyCompleted
							}

							ConsumerCompleteJobResult::InvalidBalanceDelta { message } => {
								CompleteJobResponse::InvalidBalanceDelta { message }
							}
						}
					}

					None => CompleteJobResponse::InvalidConsumerPeerId {
						message: "P2P node is not running".to_string(),
					},
				}
			};

			let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
				id: request.id,
				data: OutboundResponseFrameData::CompleteJob(response),
			});

			let _ = outbound_tx.send(outbound_frame).await;
		}

		InboundRequestFrameData::ConfirmJobCompletion { database_job_id } => {
			type ConsumerGetCompletedJobResult = crate::database::service_jobs::consumer::get_unconfirmed::ConsumerGetCompletedJobResult;

			let response = match consumer_get_unconfirmed_job(
				&mut *state.database.lock().await,
				database_job_id,
			) {
				ConsumerGetCompletedJobResult::Ok {
					provider_peer_id,
					consumer_peer_id,
					provider_job_id,
					job_hash,
					consumer_signature,
				} => match state.p2p.lock().await.public_key() {
					Some(public_key) => {
						if consumer_peer_id != public_key.to_peer_id() {
							Some(ConfirmJobCompletionResponse::InvalidConsumerPeerId {
								message: "Local peer ID doesn't match the job's".to_string(),
							})
						} else {
							// We need to send a P2P message, which may take some time.
							// Therefore, we do it in background.
							//

							let future = p2p_confirm_job_completion(
								state.clone(),
								request.id,
								outbound_tx.clone(),
								database_job_id,
								provider_peer_id,
								public_key,
								provider_job_id,
								job_hash,
								consumer_signature,
							);

							tokio::spawn(future);

							None
						}
					}

					None => todo!(),
				},

				ConsumerGetCompletedJobResult::InvalidJobId => {
					Some(ConfirmJobCompletionResponse::InvalidJobId)
				}

				ConsumerGetCompletedJobResult::NotCompletedYet => {
					Some(ConfirmJobCompletionResponse::NotCompletedYet)
				}

				ConsumerGetCompletedJobResult::AlreadyFailed => {
					Some(ConfirmJobCompletionResponse::AlreadyFailed)
				}

				ConsumerGetCompletedJobResult::AlreadyConfirmed => {
					Some(ConfirmJobCompletionResponse::AlreadyConfirmed)
				}
			};

			if let Some(response) = response {
				let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
					id: request.id,
					data: OutboundResponseFrameData::ConfirmJobCompletion(response),
				});

				let _ = outbound_tx.send(outbound_frame).await;
			}
		}

		InboundRequestFrameData::FailJob {
			database_job_id,
			reason,
			reason_class,
			private_payload,
		} => {
			type Error = crate::database::service_jobs::fail::FailJobError;

			let response = match fail_job(
				&mut *state.database.lock().await,
				database_job_id,
				reason,
				reason_class,
				private_payload,
			) {
				Ok(_) => FailJobResponse::Ok,
				Err(Error::InvalidJobId) => FailJobResponse::InvalidJobId,
				Err(Error::AlreadyCompleted) => FailJobResponse::AlreadyCompleted,
				Err(Error::AlreadyFailed) => FailJobResponse::AlreadyFailed,
			};

			let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
				id: request.id,
				data: OutboundResponseFrameData::FailJob(response),
			});

			let _ = outbound_tx.send(outbound_frame).await;
		}
	}
}

async fn handle_open_connection_request(
	state: &Arc<SharedState>,
	outbound_tx: &tokio::sync::mpsc::Sender<OutboundFrame>,
	request_id: u32,
	offer_snapshot_id: i64,
	currency: Currency,
	future_connections: ArcMutex<HashMap<i64, libp2p::Stream>>,
) {
	let database_lock = state.database.lock().await;

	#[derive(Clone)]
	struct OfferSnapshot {
		provider_peer_id: libp2p::PeerId,
		protocol_id: String,
		offer_id: String,
		protocol_payload: String,
	}

	// REFACTOR: Move to the database mod.
	let offer_snapshot = database_lock
		.query_row(
			r#"
				SELECT
					provider_peer_id, -- #0
					protocol_id,      -- #1
					offer_id,         -- #2
					protocol_payload  -- #3
				FROM
					offer_snapshots
				WHERE
					ROWID = ?1 AND
					active = 1
			"#,
			[offer_snapshot_id],
			|row| {
				Ok(OfferSnapshot {
					provider_peer_id: libp2p::PeerId::from_str(
						row.get_ref(0).unwrap().as_str().unwrap(),
					)
					.unwrap(),
					protocol_id: row.get(1)?,
					offer_id: row.get(2)?,
					protocol_payload: row.get(3)?,
				})
			},
		)
		.optional()
		.unwrap();

	drop(database_lock);

	if let Some(offer_snapshot) = offer_snapshot {
		let provider_peer_id = offer_snapshot.provider_peer_id;

		let outbound_tx = outbound_tx.clone();
		let state = state.clone();

		tokio::spawn(async move {
			let (p2p_stream_tx, p2p_stream_rx) =
				tokio::sync::oneshot::channel::<OutboundStreamRequestResult>();

			let stream_request_tx = state.p2p.lock().await.stream_request_tx.clone();

			stream_request_tx
				.try_send(p2p::OutboundStreamRequest {
					target_peer_id: provider_peer_id,
					head_request: p2p::proto::stream::HeadRequest::ServiceConnection {
						protocol_id: offer_snapshot.protocol_id,
						offer_id: offer_snapshot.offer_id,
						protocol_payload: offer_snapshot.protocol_payload,
						currency,
					},
					result_tx: p2p_stream_tx,
				})
				.unwrap(); // Otherwise the program is not healthy.

			let open_connection_response = match p2p_stream_rx.await.unwrap() {
				Ok((consumer_peer_id, stream)) => {
					// Here, we've opened & written header to the P2P stream.
					// Shall check for the header response.
					//

					let mut stream = stream.compat();

					match read_cbor::<_, p2p::proto::stream::HeadResponse>(&mut stream)
						.await
					{
						Ok(Some(head_response)) => {
							match head_response {
								p2p::proto::stream::HeadResponse::ServiceConnection(
									service_connection_head_response,
								) => {
									type ServiceConnectionHeadResponse =
										p2p::proto::stream::ServiceConnectionHeadResponse;

									match service_connection_head_response {
										ServiceConnectionHeadResponse::Ok => {
											// We'll now create a local service connection.
											//

											let (_, connection_rowid) =
												service_connections::create_service_connection(
													&mut *state.database.lock().await,
													Left(offer_snapshot_id),
													consumer_peer_id,
													currency,
												);

											future_connections
												.lock()
												.await
												.insert(connection_rowid, stream.into_inner());

											// Finally!
											OpenConnectionResponse::Ok {
												connection_id: connection_rowid,
											}
										}
										ServiceConnectionHeadResponse::OfferNotFoundError => {
											OpenConnectionResponse::ProviderOfferNotFoundError
										}
										ServiceConnectionHeadResponse::AnotherError(e) => {
											OpenConnectionResponse::OtherRemoteError(e)
										}
									}
								}
							}
						}

						Ok(None) => {
							log::debug!(
								"P2P stream EOF'ed before we could read head response"
							);

							OpenConnectionResponse::ProviderUnreacheableError
						}

						Err(e) => {
							log::warn!(
								"While reading head response from P2P stream: {:?}",
								e
							);

							OpenConnectionResponse::OtherLocalError(format!("{}", e))
						}
					}
				}

				Err(e) => {
					log::warn!("{:?}", e);
					OpenConnectionResponse::ProviderUnreacheableError
				}
			};

			let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
				id: request_id,
				data: OutboundResponseFrameData::OpenConnection(
					open_connection_response,
				),
			});

			outbound_tx.send(outbound_frame).await.unwrap();
		});
	} else {
		log::warn!(
			"Could not find offer snapshot locally by ID {}",
			offer_snapshot_id
		);

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::OpenConnection(
				OpenConnectionResponse::OtherLocalError(
					"Could not find offer snapshot locally".to_owned(),
				),
			),
		});

		outbound_tx.send(outbound_frame).await.unwrap();
	}
}

#[allow(clippy::too_many_arguments)]
async fn p2p_confirm_job_completion(
	state: Arc<SharedState>,
	domain_request_id: u32,
	rpc_outbound_tx: tokio::sync::mpsc::Sender<OutboundFrame>,
	job_rowid: i64,
	job_provider_peer_id: libp2p::PeerId,
	job_consumer_public_key: libp2p::identity::PublicKey,
	provider_job_id: String,
	job_hash: Vec<u8>,
	job_consumer_signature: Vec<u8>,
) {
	let (response_tx, response_rx) = tokio::sync::oneshot::channel();

	let reqres_request = OutboundReqResRequestEnvelope {
		request: p2p::proto::request_response::Request::ConfirmJobCompletion {
			provider_job_id,
			job_hash,
			consumer_signature: job_consumer_signature,
			consumer_public_key: job_consumer_public_key.encode_protobuf(),
		},
		response_tx,
		target_peer_id: job_provider_peer_id,
	};

	state
		.p2p
		.lock()
		.await
		.reqres_request_tx
		.send(reqres_request)
		.await
		.unwrap();

	type ReqResResponse = p2p::proto::request_response::Response;
	type ConfirmJobCompletionReqResResponse =
		p2p::proto::request_response::ConfirmJobCompletionResponse;

	let response = match response_rx.await.unwrap() {
		Ok(response) => match response {
			ReqResResponse::ConfirmJobCompletion(response) => match response {
				ConfirmJobCompletionReqResResponse::Ok => {
					type ConsumerConfirmJobResult = crate::database::service_jobs::consumer::confirm::ConsumerConfirmJobResult;

					match consumer_confirm_job(
						&mut *state.database.lock().await,
						job_rowid,
					) {
						ConsumerConfirmJobResult::Ok => ConfirmJobCompletionResponse::Ok,

						ConsumerConfirmJobResult::InvalidJobId => {
							unreachable!("Job ID must be valid at this point")
						}

						ConsumerConfirmJobResult::NotSignedYet => {
							unreachable!("Job must be signed at this point")
						}

						ConsumerConfirmJobResult::AlreadyConfirmed => {
							log::warn!(
								"🤔 Service job #{} was confirmed before the P2P request completed",
								job_rowid
							);

							ConfirmJobCompletionResponse::Ok
						}
					}
				}

				ConfirmJobCompletionReqResResponse::AlreadyConfirmed => {
					log::warn!(
						"🤔 Service job #{} is already confirmed on the Provider side",
						job_rowid
					);

					ConfirmJobCompletionResponse::AlreadyConfirmed
				}

				error => {
					type ConsumerSetJobConfirmationErrorResult = crate::database::service_jobs::set_confirmation_error::SetJobConfirmationErrorResult;

					let confirmation_error = format!("{:?}", error);

					match set_job_confirmation_error(
						&mut *state.database.lock().await,
						job_rowid,
						&confirmation_error,
					) {
						ConsumerSetJobConfirmationErrorResult::Ok => {
							ConfirmJobCompletionResponse::ProviderError(confirmation_error)
						}

						ConsumerSetJobConfirmationErrorResult::InvalidJobId => {
							unreachable!("Job ID must be valid at this point")
						}

						ConsumerSetJobConfirmationErrorResult::AlreadyConfirmed => {
							log::warn!(
								"😮 Service job #{} was confirmed during setting confirmation error",
								job_rowid
							);

							ConfirmJobCompletionResponse::AlreadyConfirmed
						}
					}
				}
			},
		},

		Err(err) => {
			log::warn!("Failed to send ConfirmJobCompletion request: {:?}", err);
			ConfirmJobCompletionResponse::ProviderUnreacheable
		}
	};

	let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
		id: domain_request_id,
		data: OutboundResponseFrameData::ConfirmJobCompletion(response),
	});

	rpc_outbound_tx.send(outbound_frame).await.unwrap();
}
