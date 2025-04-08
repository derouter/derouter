use std::sync::Arc;

use either::Either::{Left, Right};
use eyre::eyre;
use futures::AsyncWriteExt;
use libp2p::{PeerId, Stream, StreamProtocol};
use libp2p_stream::{Control, OpenStreamError};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::{
	db::create_service_connection,
	p2p::STREAM_PROTOCOL,
	rpc,
	state::{ProvidedOffer, ProviderOutboundRequestEnvelope, SharedState},
	util::cbor::{read_cbor, write_cbor},
};

use super::{Node, proto};

/// `(our_peer_id, stream)`.
pub type OutboundStreamRequestResult =
	Result<(PeerId, Stream), OpenStreamError>;

pub struct OutboundStreamRequest {
	pub target_peer_id: PeerId,
	pub head_request: proto::stream::HeadRequest,
	pub result_tx: tokio::sync::oneshot::Sender<OutboundStreamRequestResult>,
}

impl Node {
	pub(super) async fn handle_outbound_stream_request(
		our_peer_id: PeerId,
		mut control: Control,
		request: OutboundStreamRequest,
	) {
		let result =
			Self::open_outbound_stream(our_peer_id, &mut control, &request).await;
		let _ = request.result_tx.send(result);
	}

	async fn open_outbound_stream(
		our_peer_id: PeerId,
		control: &mut Control,
		request: &OutboundStreamRequest,
	) -> Result<(PeerId, Stream), OpenStreamError> {
		let mut stream = control
			.open_stream(request.target_peer_id, StreamProtocol::new(STREAM_PROTOCOL))
			.await?;

		let header_buffer = serde_cbor::to_vec(&request.head_request).unwrap();
		let len_buffer = (header_buffer.len() as u32).to_be_bytes();

		stream
			.write_all(&len_buffer)
			.await
			.map_err(libp2p_stream::OpenStreamError::Io)?;

		stream
			.write_all(&header_buffer)
			.await
			.map_err(libp2p_stream::OpenStreamError::Io)?;

		log::debug!("Successfully written {:?}", request.head_request);

		Ok((our_peer_id, stream))
	}

	pub(super) async fn handle_incoming_stream(
		state: Arc<SharedState>,
		from_peer_id: PeerId,
		to_peer_id: PeerId,
		stream: Stream,
	) -> eyre::Result<()> {
		log::info!("🌊 Incoming stream from {:?}", from_peer_id);
		let mut stream = stream.compat();

		let header: proto::stream::HeadRequest =
			match read_cbor(&mut stream).await? {
				Some(header) => {
					log::debug!("Read {:?}", header);
					header
				}

				None => {
					log::warn!("P2P stream EOF'ed before header is read");
					return Ok(());
				}
			};

		match header {
			proto::stream::HeadRequest::ServiceConnection {
				protocol_id,
				offer_id,
				protocol_payload,
				currency,
			} => {
				let provider_lock = state.provider.lock().await;
				let mut found_offer: Option<(u32, ProvidedOffer)> = None;

				if let Some(module_ids_by_protocol_id) =
					provider_lock.offers_module_map.get(&protocol_id)
				{
					let module_id = module_ids_by_protocol_id.get(&offer_id);

					if let Some(module_id) = module_id {
						found_offer = Some((
							*module_id,
							provider_lock
								.modules
								.get(module_id)
								.expect("provider module offers to be in sync")
								.offers
								.get(&protocol_id)
								.expect("provider module offers to be in sync")
								.get(&offer_id)
								.expect("provider module offers to be in sync")
								.clone(),
						));
					}
				}

				drop(provider_lock);

				type ServiceConnectionHeadResponse =
					proto::stream::ServiceConnectionHeadResponse;

				let response = if let Some((_, ref offer)) = found_offer {
					if offer.protocol_payload == *protocol_payload {
						log::debug!("🤝 Authorized incoming P2P stream");
						ServiceConnectionHeadResponse::Ok
					} else {
						log::debug!(
							"Protocol payload mismatch: {} vs {}",
							offer.protocol_payload,
							protocol_payload
						);

						ServiceConnectionHeadResponse::OfferNotFoundError
					}
				} else {
					log::debug!(
						r#"Could not find offer "{}" => "{}""#,
						protocol_id,
						offer_id
					);

					ServiceConnectionHeadResponse::OfferNotFoundError
				};

				let head_response =
					proto::stream::HeadResponse::ServiceConnection(response.clone());

				log::debug!("{:?}", head_response);
				write_cbor(&mut stream, &head_response).await??;

				let (module_id, mut offer) = match response {
					proto::stream::ServiceConnectionHeadResponse::Ok => {
						found_offer.unwrap()
					}
					_ => return Ok(()),
				};

				let mut conn = state.db.lock().await;

				let offer_snapshot = if let Some(snapshot_rowid) = offer.snapshot_rowid
				{
					// If the provided offer is saved to DB, reuse its ROWID.
					Left(snapshot_rowid)
				} else {
					// Otherwise, insert a new offer snapshot.
					// See `create_service_connection` for details.
					Right((to_peer_id, &offer_id, &protocol_id, &offer.protocol_payload))
				};

				let (offer_snapshot_rowid, connection_rowid) =
					create_service_connection(
						&mut conn,
						offer_snapshot,
						from_peer_id,
						currency,
					);

				drop(conn);

				// Mark the snapshot as saved in DB.
				offer.snapshot_rowid = Some(offer_snapshot_rowid);

				let mut provider_lock = state.provider.lock().await;

				// The module may be gone by this moment.
				let module = match provider_lock.modules.get_mut(&module_id) {
					Some(x) => x,
					None => {
						return Err(eyre!("Provider module #{} is gone", module_id));
					}
				};

				// We don't really care about the ACK response.
				let (response_tx, _) = tokio::sync::oneshot::channel();

				let envelope = ProviderOutboundRequestEnvelope {
					frame_data: rpc::OutboundRequestFrameData::ProviderOpenConnection {
						customer_peer_id: from_peer_id.to_base58(),
						protocol_id,
						offer_id,
						protocol_payload: offer.protocol_payload,
						connection_id: connection_rowid,
					},
					response_tx,
				};

				module
					.outbound_request_tx
					.try_send(envelope)
					.expect("module's outbound_request_tx should be healthy");

				module
					.future_service_connections
					.lock()
					.await
					.insert(connection_rowid, stream.into_inner());

				drop(provider_lock);

				log::debug!(
					"⏳ Provider module #{} is waiting a Yamux stream \
					for service connection #{}...",
					module_id,
					connection_rowid
				);

				Ok(())
			}
		}
	}
}
