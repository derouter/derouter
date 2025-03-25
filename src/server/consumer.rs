use std::{net::SocketAddr, sync::Arc};

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
			config::ConsumerConfigResponse,
		},
	},
};
use tokio::{
	io::AsyncWriteExt as _,
	net::TcpStream,
	sync::{Mutex, broadcast::error::RecvError},
};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::{
	database::{fetch_offers, fetch_providers},
	state::{ConsumerNotification, SharedState},
	util::{
		self,
		cbor::{CborReader, write_cbor},
	},
};

mod rpc;

pub async fn handle_connection(
	stream: TcpStream,
	_addr: SocketAddr,
	state: &SharedState,
) {
	let (rpc_stream_tx, rpc_stream_rx) =
		tokio::sync::oneshot::channel::<yamux::Stream>();

	let rpc_stream_tx = Arc::new(Mutex::new(Some(rpc_stream_tx)));

	let _ = util::yamux::YamuxServer::new(stream, None, move |stream| {
		log::debug!("New yamux stream ({})", stream.id());

		let rpc_stream_tx = rpc_stream_tx.clone();

		async move {
			let mut rpc_stream_tx = rpc_stream_tx.lock().await;

			if let Some(rpc_stream_tx) = rpc_stream_tx.take() {
				log::debug!("Assigned RPC stream ({})", stream.id());
				rpc_stream_tx.send(stream).unwrap();
				return Ok(());
			}

			Ok(())
		}
	});

	let rpc_stream = rpc_stream_rx.await.unwrap();
	let mut outbound_requests_counter = 0u32;
	let mut cbor_reader = CborReader::new(rpc_stream.compat());
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
						handle_request(state, &outbound_tx, &mut config, request).await;
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
}

async fn handle_request(
	state: &SharedState,
	outbound_tx: &tokio::sync::mpsc::Sender<OutboundFrame>,
	config: &mut Option<ConsumerConfig>,
	request: InboundRequestFrame,
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

				OutboundFrame::Response(OutboundResponseFrame {
					id: request.id,
					data: OutboundResponseFrameData::Config(ConsumerConfigResponse {
						providers: fetch_providers(&database),
						offers: fetch_offers(&database),
					}),
				})
			};

			outbound_tx.send(outbound_frame).await.unwrap();
		}

		InboundRequestFrameData::OpenConnection(_) => todo!(),
	}
}
