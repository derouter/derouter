use std::sync::Arc;

use crate::{
	db::{
		self,
		service_jobs::{
			consumer::confirm::consumer_confirm_job, query_all::UnconfirmedJob,
			set_confirmation_error::set_job_confirmation_error,
		},
	},
	p2p::proto::{
		self,
		request_response::{ConfirmJobRequst, ConfirmJobResponse, Response},
	},
	state::{RpcEvent, SharedState},
};

use super::reqres::OutboundRequestEnvelope;

/// Run the job confirmation loop: it'd send
/// job confirmations to according Providers.
///
/// NOTE: We can't and won't send signatures for jobs
/// which consumer peer ID differs from the current one.
pub async fn job_confirmation_loop(state: Arc<SharedState>) {
	let p2p_lock = state.p2p.lock().await;

	let public_key = p2p_lock.keypair.public();

	let peer_id = public_key.to_peer_id();
	let peers = p2p_lock.peers.clone();
	let peers_notify = p2p_lock.peers_notify.clone();
	let shutdown_token = state.shutdown_token.clone();
	drop(p2p_lock);

	let job_completion_notify = state.consumer.job_completion_notify.clone();

	loop {
		for job in db::query_unconfirmed_jobs(&*state.db.lock().await, peer_id) {
			if peers.read().await.contains_key(&job.provider_peer_id) {
				tokio::spawn(confirm_job(state.clone(), job, public_key.clone()));
			}
		}

		tokio::select! {
			_ = shutdown_token.cancelled() => {
				break;
			}

			_ = job_completion_notify.notified() => {
				continue;
			}

			_ = peers_notify.notified() => {
				continue;
			}
		}
	}
}

async fn confirm_job(
	state: Arc<SharedState>,
	job: UnconfirmedJob,
	public_key: libp2p::identity::PublicKey,
) {
	let (response_tx, response_rx) = tokio::sync::oneshot::channel();

	state
		.p2p
		.lock()
		.await
		.reqres_request_tx
		.send(OutboundRequestEnvelope {
			request: proto::request_response::Request::ConfirmJob(ConfirmJobRequst {
				provider_job_id: job.provider_job_id,
				job_hash: job.hash,
				consumer_public_key: public_key.encode_protobuf(),
				consumer_signature: job.consumer_signature,
			}),
			response_tx,
			target_peer_id: job.provider_peer_id,
		})
		.await
		.unwrap();

	match response_rx.await.unwrap() {
		Ok(response) => {
			let error = if let Response::ConfirmJob(response) = response {
				match response {
					ConfirmJobResponse::Ok => {
						type ConsumerConfirmJobError =
            crate::db::service_jobs::consumer::confirm::ConsumerConfirmJobError;

						match consumer_confirm_job(&mut *state.db.lock().await, job.rowid) {
							Ok(job_record) => {
								log::info!("✅ Successfully confirmed job #{}", job.rowid);

								let _ = state
									.rpc
									.lock()
									.await
									.event_tx
									.send(RpcEvent::JobUpdated(Box::new(job_record)));

								Ok(())
							}

							Err(ConsumerConfirmJobError::InvalidJobId) => {
								unreachable!("Job ID must be valid at this point")
							}

							Err(ConsumerConfirmJobError::NotSignedYet) => {
								unreachable!("Job must be signed at this point")
							}

							Err(ConsumerConfirmJobError::AlreadyConfirmed) => {
								log::warn!(
									"🤔 Service job #{} was confirmed \
                  before the P2P request completed",
									job.rowid
								);

								Ok(())
							}
						}
					}

					ConfirmJobResponse::AlreadyConfirmed => {
						log::warn!(
							"👌 Service job #{} is already confirmed \
              on the Provider side",
							job.rowid
						);

						Ok(())
					}

					error => Err(error),
				}
			} else {
				log::warn!(
					"Unexpected Provider response \
          to ConsumerConfirmJob: {response:?}"
				);

				Err(ConfirmJobResponse::UnexpectedResponse)
			};

			if let Err(error) = error {
				type SetJobConfirmationErrorResult =
        crate::db::service_jobs::set_confirmation_error::SetJobConfirmationErrorResult;

				let confirmation_error = format!("{:?}", error);

				match set_job_confirmation_error(
					&mut *state.db.lock().await,
					job.rowid,
					&confirmation_error,
				) {
					SetJobConfirmationErrorResult::Ok => {
						log::debug!(
							"Set job #{} confirmation error: {}",
							job.rowid,
							confirmation_error
						);
					}

					SetJobConfirmationErrorResult::InvalidJobId => {
						unreachable!("Job ID must be valid at this point")
					}

					SetJobConfirmationErrorResult::AlreadyConfirmed => {
						log::warn!(
							"😮 Service job #{} was confirmed during a \
                  `set_job_confirmation_error` call",
							job.rowid
						);
					}
				}
			}
		}

		Err(outbound_failure) => {
			log::warn!("{outbound_failure:?}");
		}
	}
}
