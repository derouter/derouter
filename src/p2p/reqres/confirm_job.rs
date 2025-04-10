use libp2p::{PeerId, request_response::ResponseChannel};

use crate::{
	db::service_jobs::{
		provider::confirm::provider_confirm_job,
		set_confirmation_error::set_job_confirmation_error,
	},
	p2p::{
		Node,
		proto::{
			self,
			request_response::{ConfirmJobRequst, ConfirmJobResponse},
		},
	},
};

impl Node {
	pub(super) async fn handle_confirm_job_request(
		&mut self,
		from_peer_id: PeerId,
		request: ConfirmJobRequst,
		response_channel: ResponseChannel<proto::request_response::Response>,
	) {
		type Error =
			crate::db::service_jobs::provider::confirm::ProviderConfirmJobError;

		let mut conn = self.state.db.lock().await;
		let our_peer_id = *self.swarm.local_peer_id();

		let response = match provider_confirm_job(
			&mut conn,
			from_peer_id,
			our_peer_id,
			&request.provider_job_id,
			&request.job_hash,
			&request.consumer_public_key,
			&request.consumer_signature,
		) {
			Ok(_) => ConfirmJobResponse::Ok,

			Err(Error::JobNotFound) => ConfirmJobResponse::JobNotFound,

			Err(Error::ConsumerPeerIdMismatch) => ConfirmJobResponse::JobNotFound,

			Err(Error::AlreadyConfirmed) => ConfirmJobResponse::AlreadyConfirmed,

			Err(Error::AlreadyFailed) => ConfirmJobResponse::AlreadyFailed,

			Err(Error::NotCompletedYet) => ConfirmJobResponse::NotCompletedYet,

			Err(Error::PublicKeyDecodingFailed(e)) => {
				log::debug!("Consumer public key {:?}", e);
				ConfirmJobResponse::InvalidPublicKey
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
							hex::encode(&request.job_hash),
							hex::encode(&expected_hash)
						),
						ConfirmJobResponse::HashMismatch(expected_hash),
					),

					Err(Error::SignatureVerificationFailed { job_rowid }) => (
						job_rowid,
						"Signature verification failed".to_string(),
						ConfirmJobResponse::InvalidSignature,
					),

					_ => unreachable!(),
				};

				type SetJobConfirmationErrorResult = crate::db::service_jobs::set_confirmation_error::SetJobConfirmationErrorResult;

				match set_job_confirmation_error(
					&mut conn,
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

		let response = proto::request_response::Response::ConfirmJob(response);

		log::debug!("{response:?}");

		match self
			.swarm
			.behaviour_mut()
			.request_response
			.send_response(response_channel, response)
		{
			Ok(_) => {}
			Err(response) => {
				log::warn!("Failed to send {:?}", response)
			}
		}
	}
}
