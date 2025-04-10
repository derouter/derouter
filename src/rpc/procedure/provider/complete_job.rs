use serde::{Deserialize, Serialize};

use crate::{
	db::service_jobs::provider::complete::provider_complete_job,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	util::format_secret_option,
};

/// Mark a job as completed, locally.
#[derive(Deserialize, derive_more::Debug)]
pub struct ProviderCompleteJobRequest {
	provider_peer_id: libp2p::PeerId,
	provider_job_id: String,

	/// Balance delta in currency-specific formatting.
	balance_delta: Option<String>,

	/// Potentially-publicly-accessible job payload,
	/// to be signed by the Consumer.
	#[debug(skip)]
	public_payload: String,

	/// Private job payload, stored locally.
	/// Would be overwritten if already set.
	#[debug("{}", format_secret_option(private_payload))]
	private_payload: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ProviderCompleteJobResponse {
	Ok {
		completed_at_sync: i64,
	},

	JobNotFound,

	AlreadyCompleted {
		completed_at_sync: i64,
	},

	AlreadyFailed {
		reason: String,
		reason_class: Option<i64>,
	},

	InvalidBalanceDelta,
}

impl Connection {
	pub async fn handle_provider_complete_job_request(
		&self,
		request_id: u32,
		request: ProviderCompleteJobRequest,
	) {
		type Error =
			crate::db::service_jobs::provider::complete::ProviderCompleteJobError;

		let response = {
			let mut db_lock = self.state.db.lock().await;

			match provider_complete_job(
				&mut db_lock,
				request.provider_peer_id,
				&request.provider_job_id,
				request.balance_delta.as_deref(),
				request.private_payload.as_deref(),
				&request.public_payload,
			) {
				Ok(job_record) => {
					let completed_at_sync = job_record.completed_at_sync.unwrap();
					ProviderCompleteJobResponse::Ok { completed_at_sync }
				}

				Err(Error::JobNotFound) => ProviderCompleteJobResponse::JobNotFound,

				Err(Error::InvalidBalanceDelta) => {
					ProviderCompleteJobResponse::InvalidBalanceDelta
				}

				Err(Error::AlreadyFailed {
					reason,
					reason_class,
				}) => ProviderCompleteJobResponse::AlreadyFailed {
					reason,
					reason_class,
				},

				Err(Error::AlreadyCompleted { completed_at_sync }) => {
					ProviderCompleteJobResponse::AlreadyCompleted { completed_at_sync }
				}
			}
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ProviderCompleteJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
