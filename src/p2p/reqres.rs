use libp2p::{PeerId, request_response::OutboundFailure};

use super::{
	Node,
	proto::{self, request_response::Request},
};

mod confirm_job;
mod create_job;
mod get_job;

pub type InboundResponse =
	Result<proto::request_response::Response, OutboundFailure>;

/// An outbound Request-Response protocol request envelope.
pub struct OutboundRequestEnvelope {
	pub target_peer_id: PeerId,
	pub request: proto::request_response::Request,
	pub response_tx: tokio::sync::oneshot::Sender<InboundResponse>,
}

impl OutboundRequestEnvelope {
	pub fn new(
		request: proto::request_response::Request,
		target_peer_id: PeerId,
	) -> (Self, tokio::sync::oneshot::Receiver<InboundResponse>) {
		let (response_tx, response_rx) = tokio::sync::oneshot::channel();

		(
			Self {
				target_peer_id,
				request,
				response_tx,
			},
			response_rx,
		)
	}
}

impl Node {
	pub async fn handle_reqres_event(
		&mut self,
		event: libp2p::request_response::Event<
			proto::request_response::Request,
			proto::request_response::Response,
		>,
	) {
		match event {
			libp2p::request_response::Event::Message {
				peer,
				connection_id,
				message,
			} => match message {
				libp2p::request_response::Message::Request {
					request, channel, ..
				} => match request {
					Request::CreateJob(request) => {
						self.handle_create_job_request(peer, request, channel).await
					}

					Request::GetJob(request) => {
						self.handle_get_job_request(peer, request, channel).await
					}

					Request::ConfirmJob(request) => {
						self
							.handle_confirm_job_request(peer, request, channel)
							.await
					}
				},

				libp2p::request_response::Message::Response {
					request_id,
					response,
				} => {
					log::debug!(
						"ReqRes incoming Response {{ peer: {peer}, \
						connection_id: {connection_id}, \
						request_id: {request_id}, \
						response: {:?} }}",
						response
					);

					let tx = self
						.response_tx_map
						.remove(&request_id)
						.expect("should have response receiver in the map");

					let _ = tx.send(InboundResponse::Ok(response));
				}
			},

			libp2p::request_response::Event::OutboundFailure {
				peer,
				connection_id,
				request_id,
				error,
			} => {
				log::warn!(
					"ReqRes OutboundFailure {{ \
					peer: {peer}, \
					connection_id: {connection_id},\
					request_id: {request_id}, \
					error: {error} }}"
				);

				let tx = self
					.response_tx_map
					.remove(&request_id)
					.expect("should have response receiver in the map");

				let _ = tx.send(InboundResponse::Err(error));
			}

			libp2p::request_response::Event::InboundFailure { .. } => {
				log::warn!("ReqRes {:?}", event);
			}

			libp2p::request_response::Event::ResponseSent { .. } => {
				log::debug!("ReqRes {:?}", event);
			}
		};
	}
}
