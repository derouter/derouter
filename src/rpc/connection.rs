use std::{collections::HashMap, sync::Arc};

use crate::{
	rpc::procedure::{
		InboundFrame, InboundRequestFrameData, InboundResponseFrameData,
		OutboundFrame, OutboundRequestFrame, OutboundRequestFrameData,
	},
	util::ArcMutex,
};
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	net::TcpStream,
};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::{
	state::{RpcEvent, SharedState},
	util::{
		self,
		cbor::{CborBufReader, write_cbor},
		to_arc_mutex,
	},
};

use super::{OutboundRequestEnvelope, procedure::InboundRequestFrame};

pub struct Connection {
	pub state: Arc<SharedState>,
	pub outbound_tx: tokio::sync::mpsc::Sender<OutboundFrame>,
	pub subscriptions_counter: u32,
	pub subscriptions: HashMap<u32, Subscription>,
	pub outbound_request_tx: tokio::sync::mpsc::Sender<OutboundRequestEnvelope>,
	pub future_connection_counter: ArcMutex<u64>,
	pub future_connections: ArcMutex<HashMap<u64, libp2p::Stream>>,
	pub provider_module_id: Option<u32>,
}

pub enum Subscription {
	Jobs {
		/// If set, would filter jobs by their offers' protocol ID.
		protocol_ids: Option<Vec<String>>,

		/// If set, would filter jobs by their provider peer ID.
		provider_peer_ids: Option<Vec<libp2p::PeerId>>,

		/// If set, would filter jobs by their consumer peer ID.
		consumer_peer_ids: Option<Vec<libp2p::PeerId>>,
	},

	ActiveOffers {
		/// Optionally filter by given protocol IDs.
		protocol_ids: Option<Vec<String>>,

		/// Optionally filter by given provider peer IDs.
		provider_peer_ids: Option<Vec<libp2p::PeerId>>,
	},

	ActiveProviders,
}

impl Connection {
	#[allow(clippy::too_many_arguments)]
	async fn handle_inbound_request(&mut self, request: InboundRequestFrame) {
		match request.data {
			InboundRequestFrameData::FailJob(data) => {
				self.handle_fail_job_request(request.id, data).await;
			}

			InboundRequestFrameData::ConsumerCompleteJob(data) => {
				self
					.handle_consumer_complete_job_request(request.id, data)
					.await;
			}

			InboundRequestFrameData::ConsumerCreateJob(data) => {
				self
					.handle_consumer_create_job_request(request.id, data)
					.await;
			}

			InboundRequestFrameData::ConsumerGetJob(data) => {
				self.handle_consumer_get_job_request(request.id, data).await;
			}

			InboundRequestFrameData::ConsumerOpenJobConnection(data) => {
				self
					.handle_consumer_open_job_connection_request(request.id, data)
					.await;
			}

			InboundRequestFrameData::ProviderCompleteJob(data) => {
				self
					.handle_provider_complete_job_request(request.id, data)
					.await;
			}

			InboundRequestFrameData::ProviderProvide(data) => {
				self
					.handle_provider_provide_request(request.id, &data)
					.await;
			}

			InboundRequestFrameData::QueryJobs(data) => {
				self.handle_jobs_query_request(request.id, &data).await;
			}

			InboundRequestFrameData::QueryOfferSnapshots(data) => {
				self
					.handle_offer_snapshots_query_request(request.id, &data)
					.await;
			}

			InboundRequestFrameData::QueryActiveProviders(data) => {
				self
					.handle_active_providers_query_request(request.id, &data)
					.await;
			}

			InboundRequestFrameData::QueryActiveOffers(data) => {
				self
					.handle_active_offers_query_request(request.id, &data)
					.await;
			}

			InboundRequestFrameData::QueryProviders(data) => {
				self.handle_providers_query_request(request.id, &data).await;
			}

			InboundRequestFrameData::QuerySystem(data) => {
				self.handle_system_query_request(request.id, &data).await;
			}

			InboundRequestFrameData::CancelSubscription(data) => {
				self
					.handle_cancel_subscription_request(request.id, &data)
					.await;
			}

			InboundRequestFrameData::SubscribeToActiveOffers(data) => {
				self
					.handle_active_offers_subscription_request(request.id, &data)
					.await;
			}

			InboundRequestFrameData::SubscribeToActiveProviders(data) => {
				self
					.handle_active_providers_subscription_request(request.id, &data)
					.await;
			}

			InboundRequestFrameData::SubscribeToJobs(data) => {
				self
					.handle_jobs_subscription_request(request.id, &data)
					.await;
			}
		}
	}

	async fn handle_event(&self, event: RpcEvent) {
		let mut frame_datas: Vec<OutboundRequestFrameData> = vec![];

		fn check_against_offer_subscription(
			subscription_id: u32,
			subscription_protocol_ids: &Option<Vec<String>>,
			subscription_provider_peer_ids: &Option<Vec<libp2p::PeerId>>,
			offer_protocol_id: &str,
			offer_provider_peer_id: libp2p::PeerId,
		) -> bool {
			if let Some(protocol_ids) = subscription_protocol_ids {
				if !protocol_ids.iter().any(|p| p == offer_protocol_id) {
					log::debug!(
						r#"ActiveOffers subscription #{} \
							protocol ID mismatch: {}"#,
						subscription_id,
						offer_protocol_id
					);

					return false;
				}
			} else if let Some(provider_peer_ids) = subscription_provider_peer_ids {
				if !provider_peer_ids
					.iter()
					.any(|p| *p == offer_provider_peer_id)
				{
					log::debug!(
						r#"ActiveOffers subscription #{} \
							provider peer ID mismatch: {}"#,
						subscription_id,
						offer_provider_peer_id
					);

					return false;
				}
			}

			true
		}

		match event {
			RpcEvent::OfferRemoved(data) => {
				for (subscription_id, subscription) in &self.subscriptions {
					if let Subscription::ActiveOffers {
						protocol_ids,
						provider_peer_ids,
					} = subscription
					{
						if !check_against_offer_subscription(
							*subscription_id,
							protocol_ids,
							provider_peer_ids,
							&data.protocol_id,
							data.provider_peer_id,
						) {
							continue;
						}

						frame_datas.push(OutboundRequestFrameData::EventOfferRemoved {
							subscription_id: *subscription_id,
							payload: data.clone(),
						});
					}
				}
			}

			RpcEvent::OfferUpdated(data) => {
				for (subscription_id, subscription) in &self.subscriptions {
					if let Subscription::ActiveOffers {
						protocol_ids,
						provider_peer_ids,
					} = subscription
					{
						if !check_against_offer_subscription(
							*subscription_id,
							protocol_ids,
							provider_peer_ids,
							&data.protocol_id,
							data.provider_peer_id,
						) {
							continue;
						}

						frame_datas.push(OutboundRequestFrameData::EventOfferUpdated {
							subscription_id: *subscription_id,
							payload: data.clone(),
						});
					}
				}
			}

			RpcEvent::ProviderHeartbeat(data) => {
				for (subscription_id, subscription) in &self.subscriptions {
					if let Subscription::ActiveProviders = subscription {
						frame_datas.push(
							OutboundRequestFrameData::EventProviderHeartbeat {
								subscription_id: *subscription_id,
								payload: data.clone(),
							},
						);
					}
				}
			}

			RpcEvent::ProviderUpdated(data) => {
				for (subscription_id, subscription) in &self.subscriptions {
					if let Subscription::ActiveProviders = subscription {
						frame_datas.push(OutboundRequestFrameData::EventProviderUpdated {
							subscription_id: *subscription_id,
							payload: data.clone(),
						});
					}
				}
			}

			RpcEvent::JobUpdated(data) => {
				for (subscription_id, subscription) in &self.subscriptions {
					if let Subscription::Jobs {
						protocol_ids,
						provider_peer_ids,
						consumer_peer_ids,
					} = subscription
					{
						if let Some(protocol_ids) = protocol_ids {
							if !protocol_ids.iter().any(|p| *p == data.offer_protocol_id) {
								log::debug!(
									r#"Jobs subscription #{} \
     											protocol ID mismatch: {}"#,
									subscription_id,
									data.offer_protocol_id
								);

								continue;
							}
						} else if let Some(provider_peer_ids) = provider_peer_ids {
							if !provider_peer_ids
								.iter()
								.any(|p| *p == data.provider_peer_id)
							{
								log::debug!(
									r#"Jobs subscription #{} \
     											provider peer ID mismatch: {}"#,
									subscription_id,
									data.provider_peer_id
								);

								continue;
							}
						} else if let Some(consumer_peer_ids) = consumer_peer_ids {
							if !consumer_peer_ids
								.iter()
								.any(|p| *p == data.consumer_peer_id)
							{
								log::debug!(
									r#"Jobs subscription #{} \
     											consumer peer ID mismatch: {}"#,
									subscription_id,
									data.consumer_peer_id
								);

								continue;
							}
						}

						frame_datas.push(OutboundRequestFrameData::EventJobUpdated {
							subscription_id: *subscription_id,
							payload: data.clone(),
						});
					}
				}
			}
		};

		for frame_data in frame_datas {
			let (outbound_frame, _) = OutboundRequestEnvelope::new(frame_data);
			self.outbound_request_tx.send(outbound_frame).await.unwrap();
		}
	}
}

pub async fn handle_connection(stream: TcpStream, state: Arc<SharedState>) {
	let (rpc_stream_tx, rpc_stream_rx) =
		tokio::sync::oneshot::channel::<yamux::Stream>();

	let rpc_stream_tx = to_arc_mutex(Some(rpc_stream_tx));

	let future_connections = to_arc_mutex(HashMap::<u64, libp2p::Stream>::new());
	let future_connections_clone = future_connections.clone();

	let opened_connections =
		to_arc_mutex(HashMap::<u64, tokio::task::JoinHandle<()>>::new());
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

			let connection_id = match yamux_compat.read_u64().await {
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
					"✅ Successfully joined Yamux & P2P streams for connection {}",
					connection_id
				);
			} else {
				log::warn!(
					"Local connection ID from an incoming \
					Yamux stream not found: {}",
					connection_id
				);
			}

			Ok(())
		}
	});

	let rpc_stream = match rpc_stream_rx.await {
		Ok(stream) => stream,
		Err(e) => {
			log::error!("RPC stream {e:?}, closing connection");
			return;
		}
	};

	let mut cbor_reader = CborBufReader::new(rpc_stream.compat());

	let (outbound_tx, mut outbound_rx) =
		tokio::sync::mpsc::channel::<OutboundFrame>(16);

	let mut outbound_requests_counter = 0u32;

	// NOTE: This channel is more specialized.
	let (provider_outbound_request_tx, mut provider_outbound_request_rx) =
		tokio::sync::mpsc::channel::<OutboundRequestEnvelope>(16);

	let mut inbound_response_txs = HashMap::<
		u32, // <- outbound_requests_counter
		tokio::sync::oneshot::Sender<InboundResponseFrameData>,
	>::new();

	let consumer_lock = state.rpc.lock().await;
	let mut event_rx = consumer_lock.event_tx.subscribe();
	drop(consumer_lock);

	// For `state.provider.modules`.
	let provider_module_id: Option<u32> = None;

	let subscriptions_counter: u32 = 0;
	let subscriptions = HashMap::<u32, Subscription>::new();

	let mut connection = Connection {
		state: state.clone(),
		future_connection_counter: to_arc_mutex(0),
		future_connections,
		outbound_request_tx: provider_outbound_request_tx,
		outbound_tx: outbound_tx.clone(),
		provider_module_id,
		subscriptions,
		subscriptions_counter,
	};

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
						log::debug!("⬅️ {:?}", request);

						connection.handle_inbound_request(
							request,
						).await
					},

					Ok(Some(InboundFrame::Response(response))) => {
						log::debug!("⬅️ {:?}", response);

						if let Some(inbound_response_tx) = inbound_response_txs.remove(&response.id) {
							let _ = inbound_response_tx.send(response.data);
						} else {
							log::warn!("Inbound response with unexpected ID {}", response.id);
						}
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

			envelope = provider_outbound_request_rx.recv() => {
				if let Some(envelope) = envelope {
					inbound_response_txs.insert(
						outbound_requests_counter,
						envelope.response_tx
					);

					outbound_tx.send(OutboundFrame::Request(OutboundRequestFrame {
						id: outbound_requests_counter,
						data: envelope.frame_data
					})).await.unwrap();

					outbound_requests_counter += 1;
				} else {
					log::warn!("outbound_request_rx closed, breaking loop");
					break;
				}
			}

			result = event_rx.recv() => {
				match result {
					Ok(event) => {
						connection.handle_event(event).await;
					},

					Err(tokio::sync::broadcast::error::RecvError::Lagged(e)) => {
						log::warn!("{:?}", e);
					}

					Err(e) => {
						panic!("{:?}", e);
					}
				}
			}
		}
	}

	log::debug!("Closing connection...");

	if let Some(provider_module_id) = connection.provider_module_id {
		log::debug!("Cleaning up provider module #{}...", provider_module_id);
		let mut provider_lock = state.provider.lock().await;

		let module = provider_lock.modules.remove(&provider_module_id).unwrap();

		for (protocol_id, module_offers_by_protocol) in &module.offers {
			let provider_offers_by_protocol = provider_lock
				.offers_module_map
				.get_mut(protocol_id)
				.unwrap();

			for offer_id in module_offers_by_protocol.keys() {
				log::debug!(r#"Removing offer "{}" => "{}""#, protocol_id, offer_id);
				provider_offers_by_protocol.remove(offer_id).unwrap();
			}

			if provider_offers_by_protocol.is_empty() {
				log::debug!(r#"Removing empty protocol "{}""#, protocol_id);
				provider_lock.offers_module_map.remove(protocol_id).unwrap();
			}
		}

		provider_lock.last_updated_at = chrono::Utc::now();
	}
}
