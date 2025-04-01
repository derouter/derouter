use std::{collections::HashMap, net::SocketAddr, ops::DerefMut, sync::Arc};

use rpc::{
	InboundResponseFrameData,
	inbound::{
		InboundFrame,
		request::{InboundRequestFrameData, config::ProviderConfig},
	},
	outbound::{
		OutboundFrame,
		request::OutboundRequestFrame,
		response::{
			OutboundResponseFrame, OutboundResponseFrameData,
			complete_job::CompleteJobResponse, config::ConfigResponse,
			create_job::CreateJobResponse, fail_job::FailJobResponse,
		},
	},
};
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt as _},
	net::TcpStream,
};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt};
use unwrap_none::UnwrapNone as _;

use crate::{
	database::service_jobs::{
		fail::fail_job, provider::complete::provider_complete_job,
		provider::create::provider_create_job,
	},
	state::{ProvidedOffer, ProviderOutboundRequestEnvelope, SharedState},
	util::{
		self, ArcMutex,
		cbor::{CborBufReader, write_cbor},
		to_arc_mutex,
	},
};

pub mod rpc;

// REFACTOR: Make it transport-agnostic (`stream: T: AsyncRead + AsyncWrite`).
pub async fn handle_connection(
	stream: TcpStream,
	_addr: SocketAddr,
	state: &Arc<SharedState>,
) {
	let (rpc_stream_tx, rpc_stream_rx) =
		tokio::sync::oneshot::channel::<yamux::Stream>();

	let rpc_stream_tx = to_arc_mutex(Some(rpc_stream_tx));

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

			let connection_id = match yamux_compat.read_i64().await {
				Ok(x) => x,
				Err(e) => {
					log::error!("While reading from Yamux stream: {:?}", e);
					return Ok(());
				}
			};

			if let Some(p2p_stream) =
				future_connections.lock().await.remove(&connection_id)
			{
				// Connect yamux & p2p streams.
				let handle = tokio::spawn(async move {
					match tokio::io::copy_bidirectional(
						&mut yamux_compat,
						&mut p2p_stream.compat(),
					)
					.await
					{
						Ok(_) => {
							log::debug!(
								"Both Yamux & P2P streams shut down ({})",
								connection_id
							);
						}

						Err(e) => {
							log::error!("(copy_bidirectional) {:?}", e);
						}
					}
				});

				opened_connections
					.lock()
					.await
					.insert(connection_id, handle);

				log::debug!(
					"✅ Successfully joined Yamux & P2P streams for service connection {}",
					connection_id
				);
			}

			Ok(())
		}
	});

	let rpc_stream = rpc_stream_rx.await.unwrap();
	let mut cbor_reader = CborBufReader::new(rpc_stream.compat());

	let (outbound_tx, mut outbound_rx) =
		tokio::sync::mpsc::channel::<OutboundFrame>(16);

	// NOTE: This channel is more specialized.
	let (outbound_request_tx, mut outbound_request_rx) =
		tokio::sync::mpsc::channel::<ProviderOutboundRequestEnvelope>(16);

	let mut config: Option<ProviderConfig> = None;
	let mut outbound_request_counter = 0u32;
	let mut inbound_response_txs = HashMap::<
		u32,
		tokio::sync::oneshot::Sender<InboundResponseFrameData>,
	>::new();

	loop {
		tokio::select! {
			biased;

			_ = state.shutdown_token.cancelled() => {
				log::debug!("Breaking RPC loop due to signal");
				break;
			}

			result = cbor_reader.next_cbor::<InboundFrame>() => {
				match result {
					Ok(Some(frame)) => {
						handle_inbound_frame(
							frame,
							&mut config,
							&outbound_tx,
							state,
							&outbound_request_tx,
							&future_connections,
							&mut cbor_reader,
							&mut inbound_response_txs
						).await;
					}

					Ok(None) => {
						log::debug!("Reader EOF");
						break;
					}

					Err(e) => {
						log::error!("{}", e);
					}
				}
			}

			envelope = outbound_request_rx.recv() => {
				if let Some(envelope) = envelope {
					inbound_response_txs.insert(
						outbound_request_counter,
						envelope.response_tx
					);

					outbound_tx.send(OutboundFrame::Request(OutboundRequestFrame {
						id: outbound_request_counter,
						data: envelope.frame_data
					})).await.unwrap();

					outbound_request_counter += 1;
				} else {
					log::warn!("outbound_request_rx closed, breaking loop");
					break;
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
						log::debug!("Writer EOF");
						break;
					}
				}
			}
		}
	}

	if let Some(config) = config {
		log::debug!("Cleaning up...");

		let mut lock = state.provider.lock().await;

		for config_offer in &config.offers {
			let protocol_id = &config_offer.1.protocol;
			let offer_id = config_offer.0;

			let offers_by_protocol = lock.offers.get_mut(protocol_id).unwrap();
			offers_by_protocol.remove(offer_id).unwrap();

			if offers_by_protocol.is_empty() {
				lock.offers.remove(protocol_id);
				log::debug!("Removed empty protocol hash: {}", protocol_id);
			}
		}

		lock.modules.remove(&config.provider_id);
		lock.last_updated_at = chrono::Utc::now();
	}

	for handle in opened_connections.lock().await.deref_mut().values_mut() {
		handle.abort();
	}
}

#[allow(clippy::too_many_arguments)]
async fn handle_inbound_frame(
	frame: InboundFrame,
	config: &mut Option<ProviderConfig>,
	outbound_tx: &tokio::sync::mpsc::Sender<OutboundFrame>,
	state: &Arc<SharedState>,
	outbound_request_tx: &tokio::sync::mpsc::Sender<
		ProviderOutboundRequestEnvelope,
	>,
	future_connections: &ArcMutex<HashMap<i64, libp2p::Stream>>,
	cbor_reader: &mut CborBufReader<Compat<yamux::Stream>>,
	inbound_response_txs: &mut HashMap<
		u32,
		tokio::sync::oneshot::Sender<InboundResponseFrameData>,
	>,
) -> bool {
	match frame {
		InboundFrame::Request(request) => {
			log::debug!("⬅️ {:?}", request.data);

			match request.data {
				InboundRequestFrameData::ProviderConfig(data) => {
					if config.is_some() {
						log::warn!("Drop provider due to duplicate config");

						let outbound_frame =
							OutboundFrame::Response(OutboundResponseFrame {
								id: request.id,
								data: OutboundResponseFrameData::ProviderConfig(
									ConfigResponse::AlreadyConfigured,
								),
							});

						let _ = outbound_tx.send(outbound_frame).await;

						return false; // Break the loop to drop the connection.
					}

					match apply_config(
						data,
						state,
						outbound_request_tx.clone(),
						future_connections.clone(),
					)
					.await
					{
						Ok(data) => {
							log::debug!("Config validated");

							*config = Some(data);

							let outbound_frame =
								OutboundFrame::Response(OutboundResponseFrame {
									id: request.id,
									data: OutboundResponseFrameData::ProviderConfig(
										ConfigResponse::Ok,
									),
								});

							let _ = outbound_tx.send(outbound_frame).await;
						}

						Err(response) => {
							log::warn!("Drop provider due to misconfig: {:?}", response);

							let outbound_frame =
								OutboundFrame::Response(OutboundResponseFrame {
									id: request.id,
									data: OutboundResponseFrameData::ProviderConfig(response),
								});

							log::debug!("➡️ {:?}", outbound_frame);
							let _ = write_cbor(cbor_reader.get_mut(), &outbound_frame).await;
							let _ = cbor_reader.get_mut().flush().await;

							return false; // Break the loop to drop the connection.
						}
					}
				}

				InboundRequestFrameData::ProviderCreateJob {
					connection_id,
					private_payload,
				} => {
					type ProviderCreateJobResult = crate::database::service_jobs::provider::create::ProviderCreateJobResult;

					let response = match provider_create_job(
						&mut *state.database.lock().await,
						connection_id,
						private_payload,
					) {
						ProviderCreateJobResult::Ok {
							job_rowid,
							provider_job_id,
							created_at_sync,
						} => CreateJobResponse::Ok {
							database_job_id: job_rowid,
							provider_job_id,
							created_at_sync,
						},

						ProviderCreateJobResult::ConnectionNotFound => {
							CreateJobResponse::InvalidConnectionId
						}
					};

					let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
						id: request.id,
						data: OutboundResponseFrameData::ProviderCreateJob(response),
					});

					let _ = outbound_tx.send(outbound_frame).await;
				}

				InboundRequestFrameData::ProviderCompleteJob {
					database_job_id,
					balance_delta,
					private_payload,
					public_payload,
				} => {
					type ProviderCompleteJobResult = crate::database::service_jobs::provider::complete::ProviderCompleteJobResult;

					let response = match provider_complete_job(
						&mut *state.database.lock().await,
						database_job_id,
						balance_delta,
						private_payload,
						public_payload,
					) {
						ProviderCompleteJobResult::Ok { completed_at_sync } => {
							CompleteJobResponse::Ok { completed_at_sync }
						}
						ProviderCompleteJobResult::InvalidJobId => {
							CompleteJobResponse::InvalidJobId
						}
						ProviderCompleteJobResult::InvalidBalanceDelta(message) => {
							CompleteJobResponse::InvalidBalanceDelta { message }
						}
						ProviderCompleteJobResult::AlreadyFailed => {
							CompleteJobResponse::AlreadyFailed
						}
						ProviderCompleteJobResult::AlreadyCompleted {
							completed_at_sync,
						} => CompleteJobResponse::AlreadyCompleted { completed_at_sync },
					};

					let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
						id: request.id,
						data: OutboundResponseFrameData::ProviderCompleteJob(response),
					});

					let _ = outbound_tx.send(outbound_frame).await;
				}

				InboundRequestFrameData::ProviderFailJob {
					database_job_id,
					reason,
					reason_class,
					private_payload,
				} => {
					// ADHOC: Otherwise formatting fails.
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
						data: OutboundResponseFrameData::ProviderFailJob(response),
					});

					let _ = outbound_tx.send(outbound_frame).await;
				}
			}
		}

		InboundFrame::Response(response) => {
			log::debug!("⬅️ {:?}", response.data);

			if let Some(inbound_response_tx) =
				inbound_response_txs.remove(&response.id)
			{
				let _ = inbound_response_tx.send(response.data);
			} else {
				log::warn!("Unknown inbound frame response ID {}", response.id);
			}
		}
	}

	true
}

async fn apply_config(
	config: ProviderConfig,
	state: &Arc<SharedState>,
	outbound_request_tx: tokio::sync::mpsc::Sender<
		ProviderOutboundRequestEnvelope,
	>,
	almost_opened_connections: ArcMutex<HashMap<i64, libp2p::Stream>>,
) -> Result<ProviderConfig, ConfigResponse> {
	let mut provider_lock = state.provider.lock().await;

	if provider_lock.modules.contains_key(&config.provider_id) {
		log::warn!("Provider ID already in use: {}", config.provider_id);
		return Err(ConfigResponse::IdAlreadyUsed);
	}

	for config_offer in &config.offers {
		let protocol_id = &config_offer.1.protocol;
		let offer_id = config_offer.0;

		if let Some(offers_by_id) = provider_lock.offers.get_mut(protocol_id) {
			if offers_by_id.get(offer_id).is_some() {
				log::warn!("Duplicate offer ({} => {})", protocol_id, offer_id);

				return Err(ConfigResponse::DuplicateOffer {
					protocol_id: protocol_id.clone(),
					offer_id: offer_id.clone(),
				});
			}
		}
	}

	// Config is hereby verified.
	//

	for config_offer in &config.offers {
		let protocol_id = &config_offer.1.protocol;

		let offers_by_id = match provider_lock.offers.get_mut(protocol_id) {
			Some(x) => x,
			None => &mut {
				provider_lock
					.offers
					.insert(protocol_id.clone(), HashMap::new());

				provider_lock.offers.get_mut(protocol_id).unwrap()
			},
		};

		offers_by_id
			.insert(
				config_offer.0.to_string(),
				ProvidedOffer {
					snapshot_rowid: None,
					provider_id: config.provider_id.clone(),
					protocol_payload: config_offer.1.protocol_payload.clone(),
				},
			)
			.unwrap_none();

		log::trace!("Inserted {:?}", config_offer);
	}

	provider_lock.modules.insert(
		config.provider_id.clone(),
		crate::state::ProviderModuleState {
			outbound_request_tx,
			future_service_connections: almost_opened_connections,
		},
	);

	provider_lock.last_updated_at = chrono::Utc::now();

	Ok(config)
}
