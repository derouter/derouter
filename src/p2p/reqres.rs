use libp2p::{
	PeerId,
	request_response::{InboundRequestId, OutboundFailure, ResponseChannel},
};

use crate::{
	db::service_jobs::{
		provider::confirm::provider_confirm_job,
		set_confirmation_error::set_job_confirmation_error,
	},
	p2p::proto::request_response::ConfirmJobCompletionResponse,
	state::RpcEvent,
};

use super::{Node, proto};

pub type InboundResponse =
	Result<proto::request_response::Response, OutboundFailure>;

/// An outbound Request-Response protocol request envelope.
pub struct OutboundRequestEnvelope {
	pub target_peer_id: PeerId,
	pub request: proto::request_response::Request,
	pub response_tx: tokio::sync::oneshot::Sender<InboundResponse>,
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
					request_id,
					request,
					channel,
				} => {
					self
						.handle_incoming_request(
							*self.swarm.local_peer_id(),
							peer,
							request_id,
							request,
							channel,
						)
						.await;
				}

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

	async fn handle_incoming_request(
		&mut self,
		our_peer_id: PeerId,
		from_peer_id: PeerId,
		_request_id: InboundRequestId,
		request: proto::request_response::Request,
		channel: ResponseChannel<proto::request_response::Response>,
	) {
		type Request = proto::request_response::Request;

		match request {
			Request::ConfirmJobCompletion {
				provider_job_id,
				job_hash,
				consumer_public_key,
				consumer_signature,
			} => {
				type Error =
					crate::db::service_jobs::provider::confirm::ProviderConfirmJobError;

				let mut db = self.state.database.lock().await;

				let response = match provider_confirm_job(
					&mut db,
					&from_peer_id,
					&our_peer_id,
					&provider_job_id,
					&job_hash,
					&consumer_public_key,
					&consumer_signature,
				) {
					Ok(job_record) => {
						let _ = self
							.state
							.rpc
							.lock()
							.await
							.event_tx
							.send(RpcEvent::JobUpdated(Box::new(job_record)));

						ConfirmJobCompletionResponse::Ok
					}

					Err(Error::JobNotFound) => ConfirmJobCompletionResponse::JobNotFound,

					Err(Error::ConsumerPeerIdMismatch) => {
						ConfirmJobCompletionResponse::JobNotFound
					}

					Err(Error::AlreadyConfirmed) => {
						ConfirmJobCompletionResponse::AlreadyConfirmed
					}

					Err(Error::AlreadyFailed) => {
						ConfirmJobCompletionResponse::AlreadyFailed
					}

					Err(Error::NotCompletedYet) => {
						ConfirmJobCompletionResponse::NotCompletedYet
					}

					Err(Error::PublicKeyDecodingFailed(e)) => {
						log::debug!("Consumer public key {:?}", e);
						ConfirmJobCompletionResponse::PublicKeyDecodingFailed
					}

					result => {
						let (job_rowid, confirmation_error, response) = match result {
							Err(Error::HashMismatch {
								job_rowid,
								expected_hash,
							}) => (
								job_rowid,
								format!(
									"Hash mismatch (got {}, expected {})",
									hex::encode(&job_hash),
									hex::encode(&expected_hash)
								),
								ConfirmJobCompletionResponse::HashMismatch {
									expected: expected_hash,
								},
							),

							Err(Error::SignatureVerificationFailed { job_rowid }) => (
								job_rowid,
								"Signature verification failed".to_string(),
								ConfirmJobCompletionResponse::SignatureVerificationFailed,
							),

							_ => unreachable!(),
						};

						type SetJobConfirmationErrorResult = crate::db::service_jobs::set_confirmation_error::SetJobConfirmationErrorResult;

						match set_job_confirmation_error(
							&mut db,
							job_rowid,
							&confirmation_error,
						) {
							SetJobConfirmationErrorResult::Ok => {}

							SetJobConfirmationErrorResult::InvalidJobId => {
								unreachable!("Job ID must be valid at this point")
							}

							SetJobConfirmationErrorResult::AlreadyConfirmed => {
								// We're still returning a errornous response.
								log::warn!(
									"😮 Service job #{} was confirmed during setting confirmation error",
									job_rowid
								);
							}
						}

						response
					}
				};

				let response =
					proto::request_response::Response::ConfirmJobCompletion(response);

				log::debug!("{response:?}");

				match self
					.swarm
					.behaviour_mut()
					.request_response
					.send_response(channel, response)
				{
					Ok(_) => {}
					Err(response) => {
						log::warn!("Failed to send {:?}", response)
					}
				}
			}
		}
	}
}
