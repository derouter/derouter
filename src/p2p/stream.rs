use std::sync::Arc;

use futures::AsyncWriteExt;
use libp2p::{PeerId, Stream, StreamProtocol};
use libp2p_stream::{Control, OpenStreamError};
use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::{p2p::STREAM_PROTOCOL, state::SharedState, util::cbor::read_cbor};

use super::{Node, proto};

mod job_connection;

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
	) {
		log::info!("🌊 Incoming stream from {:?}", from_peer_id);
		let mut stream = stream.compat();

		let header: proto::stream::HeadRequest = match read_cbor(&mut stream).await
		{
			Ok(Some(header)) => {
				log::debug!("Read {:?}", header);
				header
			}

			Ok(None) => {
				log::warn!("P2P stream EOF'ed before header is read");
				return;
			}

			Err(io_error) => {
				log::error!("{io_error:?}");
				return;
			}
		};

		match header {
			proto::stream::HeadRequest::JobConnection(request) => {
				Self::handle_job_connection_stream(
					state,
					from_peer_id,
					to_peer_id,
					request,
					stream,
				)
				.await
			}
		}
	}
}
