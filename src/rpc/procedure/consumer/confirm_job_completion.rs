use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::{
	database::service_jobs::{
		consumer::{
			confirm::consumer_confirm_job,
			get_unconfirmed::consumer_get_unconfirmed_job,
		},
		set_confirmation_error::set_job_confirmation_error,
	},
	p2p,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	state::{RpcEvent, SharedState},
};

/// Confirm a job completion signature with the Provider.
/// Shall be called after [CompleteJob].
#[derive(Deserialize, Debug)]
pub struct ConsumerConfirmJobCompletionRequest {
	/// Job ID returned by [`CreateJob`].
	database_job_id: i64,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConsumerConfirmJobCompletionResponse {
	Ok,

	/// Either the local P2P node is not running,
	/// or its peer ID mismatches the job's.
	InvalidConsumerPeerId {
		message: String,
	},

	InvalidJobId,
	NotCompletedYet,
	AlreadyFailed,

	/// The job's signature  is already confirmed (not a error).
	AlreadyConfirmed,

	ProviderUnreacheable,
	ProviderError(String),
}

impl Connection {
	pub async fn handle_consumer_confirm_job_completion_request(
		&self,
		request_id: u32,
		request_data: ConsumerConfirmJobCompletionRequest,
	) {
		type Result = crate::database::service_jobs::consumer::get_unconfirmed::ConsumerGetCompletedJobResult;

		let response = match consumer_get_unconfirmed_job(
			&mut *self.state.database.lock().await,
			request_data.database_job_id,
		) {
			Result::Ok {
				provider_peer_id,
				consumer_peer_id,
				provider_job_id,
				job_hash,
				consumer_signature,
			} => {
				let public_key = self.state.p2p.lock().await.keypair.public();

				if consumer_peer_id != public_key.to_peer_id() {
					Some(
						ConsumerConfirmJobCompletionResponse::InvalidConsumerPeerId {
							message: "Local peer ID doesn't match the job's".to_string(),
						},
					)
				} else {
					// We need to send a P2P message, which may take some time.
					// Therefore, we do it in background.
					//

					let future = p2p_confirm_job_completion(
						self.state.clone(),
						request_id,
						self.outbound_tx.clone(),
						request_data.database_job_id,
						provider_peer_id,
						public_key,
						provider_job_id,
						job_hash,
						consumer_signature,
					);

					tokio::spawn(future);

					None
				}
			}

			Result::InvalidJobId => {
				Some(ConsumerConfirmJobCompletionResponse::InvalidJobId)
			}

			Result::NotCompletedYet => {
				Some(ConsumerConfirmJobCompletionResponse::NotCompletedYet)
			}

			Result::AlreadyFailed => {
				Some(ConsumerConfirmJobCompletionResponse::AlreadyFailed)
			}

			Result::AlreadyConfirmed => {
				Some(ConsumerConfirmJobCompletionResponse::AlreadyConfirmed)
			}
		};

		if let Some(response) = response {
			let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
				id: request_id,
				data: OutboundResponseFrameData::ConsumerConfirmJobCompletion(response),
			});

			let _ = self.outbound_tx.send(outbound_frame).await;
		}
	}
}

#[allow(clippy::too_many_arguments)]
async fn p2p_confirm_job_completion(
	state: Arc<SharedState>,
	domain_request_id: u32,
	rpc_outbound_tx: tokio::sync::mpsc::Sender<OutboundFrame>,
	job_rowid: i64,
	job_provider_peer_id: libp2p::PeerId,
	job_consumer_public_key: libp2p::identity::PublicKey,
	provider_job_id: String,
	job_hash: Vec<u8>,
	job_consumer_signature: Vec<u8>,
) {
	let (response_tx, response_rx) = tokio::sync::oneshot::channel();

	let reqres_request = p2p::reqres::OutboundRequestEnvelope {
		request: p2p::proto::request_response::Request::ConfirmJobCompletion {
			provider_job_id,
			job_hash,
			consumer_signature: job_consumer_signature,
			consumer_public_key: job_consumer_public_key.encode_protobuf(),
		},
		response_tx,
		target_peer_id: job_provider_peer_id,
	};

	state
		.p2p
		.lock()
		.await
		.reqres_request_tx
		.send(reqres_request)
		.await
		.unwrap();

	type ReqResResponse = p2p::proto::request_response::Response;
	type ConfirmJobCompletionReqResResponse =
		p2p::proto::request_response::ConfirmJobCompletionResponse;

	let response = match response_rx.await.unwrap() {
		Ok(response) => match response {
			ReqResResponse::ConfirmJobCompletion(response) => match response {
				ConfirmJobCompletionReqResResponse::Ok => {
					type Error = crate::database::service_jobs::consumer::confirm::ConsumerConfirmJobError;

					match consumer_confirm_job(
						&mut *state.database.lock().await,
						job_rowid,
					) {
						Ok(job_record) => {
							let _ = state
								.rpc
								.lock()
								.await
								.event_tx
								.send(RpcEvent::JobUpdated(Box::new(job_record)));

							ConsumerConfirmJobCompletionResponse::Ok
						}

						Err(Error::InvalidJobId) => {
							unreachable!("Job ID must be valid at this point")
						}

						Err(Error::NotSignedYet) => {
							unreachable!("Job must be signed at this point")
						}

						Err(Error::AlreadyConfirmed) => {
							log::warn!(
								"🤔 Service job #{} was confirmed before the P2P request completed",
								job_rowid
							);

							ConsumerConfirmJobCompletionResponse::Ok
						}
					}
				}

				ConfirmJobCompletionReqResResponse::AlreadyConfirmed => {
					log::warn!(
						"🤔 Service job #{} is already confirmed on the Provider side",
						job_rowid
					);

					ConsumerConfirmJobCompletionResponse::AlreadyConfirmed
				}

				error => {
					type ConsumerSetJobConfirmationErrorResult = crate::database::service_jobs::set_confirmation_error::SetJobConfirmationErrorResult;

					let confirmation_error = format!("{:?}", error);

					match set_job_confirmation_error(
						&mut *state.database.lock().await,
						job_rowid,
						&confirmation_error,
					) {
						ConsumerSetJobConfirmationErrorResult::Ok => {
							ConsumerConfirmJobCompletionResponse::ProviderError(
								confirmation_error,
							)
						}

						ConsumerSetJobConfirmationErrorResult::InvalidJobId => {
							unreachable!("Job ID must be valid at this point")
						}

						ConsumerSetJobConfirmationErrorResult::AlreadyConfirmed => {
							log::warn!(
								"😮 Service job #{} was confirmed during setting confirmation error",
								job_rowid
							);

							ConsumerConfirmJobCompletionResponse::AlreadyConfirmed
						}
					}
				}
			},
		},

		Err(err) => {
			log::warn!("Failed to send ConfirmJobCompletion request: {:?}", err);
			ConsumerConfirmJobCompletionResponse::ProviderUnreacheable
		}
	};

	let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
		id: domain_request_id,
		data: OutboundResponseFrameData::ConsumerConfirmJobCompletion(response),
	});

	rpc_outbound_tx.send(outbound_frame).await.unwrap();
}
