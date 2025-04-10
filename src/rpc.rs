pub mod connection;
pub mod procedure;
pub mod server;

pub use procedure::InboundResponseFrameData;
pub use procedure::OutboundRequestFrameData;

pub struct OutboundRequestEnvelope {
	pub frame_data: OutboundRequestFrameData,
	pub response_tx: tokio::sync::oneshot::Sender<InboundResponseFrameData>,
}

impl OutboundRequestEnvelope {
	pub fn new(
		frame_data: OutboundRequestFrameData,
	) -> (
		Self,
		tokio::sync::oneshot::Receiver<InboundResponseFrameData>,
	) {
		let (response_tx, response_rx) = tokio::sync::oneshot::channel();

		(
			Self {
				frame_data,
				response_tx,
			},
			response_rx,
		)
	}
}
