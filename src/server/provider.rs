use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use rpc::{
	inbound::{
		InboundFrame,
		request::{InboundRequestFrameData, config::ProviderConfig},
	},
	outbound::{
		OutboundFrame,
		response::{
			ConfigResponse, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
};
use tokio::{io::AsyncWriteExt as _, net::TcpStream, sync::Mutex};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use unwrap_none::UnwrapNone as _;

use crate::{
	state::ProviderOffer,
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
	all_provided_offers: ArcMutex<
		HashMap<String, HashMap<String, ProviderOffer>>,
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
	let mut config: Option<ProviderConfig> = None;

	loop {
		tokio::select! {
			biased;

			_ = signal.changed() => {
				log::debug!("Breaking RPC loop due to signal");
				break;
			}

			result = cbor_reader.next_cbor::<InboundFrame>() => {
				match result {
					Ok(Some(InboundFrame::Request(request))) => {
						match request.data {
							InboundRequestFrameData::Config(data) => {
								log::debug!("⬅️ {:?}", data);

								if config.is_some() {
									log::warn!("Drop provider due to duplicate config");

									let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
										id: request.id,
										data: OutboundResponseFrameData::Config(ConfigResponse::AlreadyConfigured)
									});

									let _ = outbound_tx.send(outbound_frame).await;

									break; // Break the loop to drop the connection.
								}

								match apply_config(data, &all_provided_offers).await {
									Ok(data) => {
										log::debug!("Config validated");

										config = Some(data);

										let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
											id: request.id,
											data: OutboundResponseFrameData::Config(ConfigResponse::Ok)
										});

										let _ = outbound_tx.send(outbound_frame).await;
									}

									Err(response) => {
										log::warn!("Drop provider due to misconfig: {:?}", response);

										let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
											id: request.id,
											data: OutboundResponseFrameData::Config(response)
										});

										log::debug!("➡️ {:?}", outbound_frame);
										let _ = write_cbor(cbor_reader.get_mut(), &outbound_frame).await;
										let _ = cbor_reader.get_mut().flush().await;

										break; // Break the loop to drop the connection.
									}
								}
							}
						}
					},

					Ok(None) => {
						log::debug!("Reader EOF");
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
						log::debug!("Writer EOF");
						break;
					}
				}
			}
		}
	}

	if let Some(config) = config {
		log::debug!("Cleaning up...");

		let mut all_offers = all_provided_offers.lock().await;

		for config_offer in &config.offers {
			let protocol_id = &config_offer.1.protocol;
			let offer_id = config_offer.0;

			all_offers
				.get_mut(protocol_id)
				.unwrap()
				.remove(offer_id)
				.unwrap();

			if all_offers.get_mut(protocol_id).iter().len() == 0 {
				all_offers.remove(protocol_id);
			}
		}
	}
}

async fn apply_config(
	config: ProviderConfig,
	all_provided_offers: &ArcMutex<
		HashMap<String, HashMap<String, ProviderOffer>>,
	>,
) -> Result<ProviderConfig, ConfigResponse> {
	let mut all_offers = all_provided_offers.lock().await;

	for config_offer in &config.offers {
		let protocol_id = &config_offer.1.protocol;
		let offer_id = config_offer.0;

		if let Some(offers_by_id) = all_offers.get_mut(protocol_id) {
			if offers_by_id.get(offer_id).is_some() {
				log::warn!("Duplicate offer {} => {}", protocol_id, offer_id);

				return Err(ConfigResponse::DuplicateOffer {
					protocol_id: protocol_id.clone(),
					offer_id: offer_id.clone(),
				});
			}
		}
	}

	// Okay, there are no duplicates. May insert now.
	//

	for config_offer in &config.offers {
		let protocol_id = &config_offer.1.protocol;

		let offers_by_id = match all_offers.get_mut(protocol_id) {
			Some(x) => x,
			None => &mut {
				all_offers.insert(protocol_id.clone(), HashMap::new());
				all_offers.get_mut(protocol_id).unwrap()
			},
		};

		offers_by_id
			.insert(
				config_offer.0.to_string(),
				ProviderOffer {
					_protocol_payload: config_offer.1.protocol_payload.clone(),
				},
			)
			.unwrap_none();

		log::trace!("Inserted {:?}", config_offer);
	}

	Ok(config)
}
