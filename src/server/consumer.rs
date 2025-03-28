use std::{
	collections::HashMap, net::SocketAddr, ops::DerefMut, str::FromStr, sync::Arc,
};

use either::Either::Left;
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
			config::ConsumerConfigResponse, open_connection::OpenConnectionResponse,
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
	database::{create_service_connection, fetch_active_providers, fetch_offers},
	p2p::{self, OutboundStreamRequestResult},
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
	match request.data {
		InboundRequestFrameData::Config(data) => {
			log::debug!("⬅️ {:?}", data);

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

		InboundRequestFrameData::OpenConnection(data) => {
			handle_open_connection_request(
				state,
				outbound_tx,
				request.id,
				data,
				future_connections.clone(),
			)
			.await;
		}
	}
}

async fn handle_open_connection_request(
	state: &Arc<SharedState>,
	outbound_tx: &tokio::sync::mpsc::Sender<OutboundFrame>,
	request_id: u32,
	data: rpc::inbound::request::open_connection::OpenConnection,
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
			[data.offer_snapshot_id],
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

											let (_, connection_rowid) = create_service_connection(
												&mut *state.database.lock().await,
												Left(data.offer_snapshot_id),
												consumer_peer_id,
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
			data.offer_snapshot_id
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
