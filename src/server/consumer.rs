use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use futures::{StreamExt as _, stream::FuturesUnordered};
use rpc::{
	inbound::{
		InboundFrame,
		request::{InboundRequestFrameData, config::ConsumerConfig},
	},
	outbound::{
		OutboundFrame,
		response::{OutboundResponseFrame, OutboundResponseFrameData},
	},
};
use tokio::{io::AsyncWriteExt as _, net::TcpStream, sync::Mutex};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::{
	state::OfferNotification,
	util::{
		self, ArcMutex,
		cbor::{CborReader, write_cbor},
	},
};

mod rpc;

pub async fn handle_connection(
	stream: TcpStream,
	_addr: SocketAddr,
	mut signal: tokio::sync::watch::Receiver<bool>,
	offer_txs: ArcMutex<
		HashMap<String, tokio::sync::broadcast::Sender<OfferNotification>>,
	>,
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
	let mut cbor_reader = CborReader::new(rpc_stream.compat());
	let (outbound_tx, mut outbound_rx) =
		tokio::sync::mpsc::channel::<OutboundFrame>(16);
	let mut offer_rxs =
		Vec::<tokio::sync::broadcast::Receiver<OfferNotification>>::new();
	let mut config: Option<ConsumerConfig> = None;

	loop {
		let mut offer_rxs_any: FuturesUnordered<_> =
			offer_rxs.iter_mut().map(|recv| recv.recv()).collect();

		tokio::select! {
			biased;

			result = signal.changed() => {
				log::debug!(
					"Breaking RPC loop: {:?}",
					result
				);

				break;
			}

			result = cbor_reader.next_cbor::<InboundFrame>() => {
				match result {
					Ok(Some(InboundFrame::Request(request))) => {
						match request.data {
							InboundRequestFrameData::Config(data) => {
								log::debug!("⬅️ {:?}", data);

								if config.is_some() {
									log::warn!("Already set config, ignoring");
									continue;
								}

								drop(offer_rxs_any);

								let mut offer_txs = offer_txs.lock().await;

								for protocol in &data.protocols {
									let offer_rx = if let Some(tx) = offer_txs.get_mut(protocol) {
										tx.subscribe()
									} else {
										let (tx, rx) = tokio::sync::broadcast::channel::<OfferNotification>(16);
										offer_txs.insert(protocol.clone(), tx);
										rx
									};

									log::debug!("Subscribed to {}", protocol);
									offer_rxs.push(offer_rx);
								}

								drop(offer_txs);

								config = Some(data);

								let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
									id:request.id,
									data: OutboundResponseFrameData::Ack
								});

								let _ = outbound_tx.send(outbound_frame).await;
							}
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

			result = offer_rxs_any.next() => {
				match result {
					Some(Ok(_)) => todo!(),
					Some(Err(_)) => todo!(),
					None => todo!(),
				}
			}
		}
	}

	if let Some(config) = config {
		let mut offer_txs = offer_txs.lock().await;

		// Clean up the protocol subscriptions.
		for protocol in &config.protocols {
			if let Some(tx) = offer_txs.get_mut(protocol) {
				if tx.receiver_count() == 0 {
					log::debug!("Removing sender for protocol: {}", protocol);
					offer_txs.remove(protocol);
				}
			}
		}

		drop(offer_txs);
	}
}
