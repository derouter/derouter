use serde::{Deserialize, Serialize};

use crate::{
	database::service_jobs::provider::complete::provider_complete_job,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	state::RpcEvent,
};

/// Mark a job as completed, locally.
#[derive(Deserialize, Debug)]
pub struct ProviderCompleteJobRequest {
	/// Local job ID.
	database_job_id: i64,

	/// Balance delta in currency-specific formatting.
	balance_delta: Option<String>,

	/// Potentially-publicly-accessible job payload,
	/// to be signed by the Consumer.
	public_payload: String,

	/// Private job payload, stored locally.
	/// Would override if already set.
	private_payload: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ProviderCompleteJobResponse {
	Ok { completed_at_sync: i64 },
	InvalidJobId,
	AlreadyCompleted { completed_at_sync: i64 },
	AlreadyFailed,
	InvalidBalanceDelta { message: String },
}

impl Connection {
	pub async fn handle_provider_complete_job_request(
		&self,
		request_id: u32,
		request_data: ProviderCompleteJobRequest,
	) {
		type Error = crate::database::service_jobs::provider::complete::ProviderCompleteJobError;

		let response = match provider_complete_job(
			&mut *self.state.database.lock().await,
			request_data.database_job_id,
			request_data.balance_delta,
			request_data.private_payload,
			request_data.public_payload,
		) {
			Ok(job_record) => {
				let completed_at_sync = job_record.completed_at_sync.unwrap();

				let _ = self
					.state
					.rpc
					.lock()
					.await
					.event_tx
					.send(RpcEvent::JobUpdated(Box::new(job_record)));

				ProviderCompleteJobResponse::Ok { completed_at_sync }
			}

			Err(Error::InvalidJobId) => ProviderCompleteJobResponse::InvalidJobId,

			Err(Error::InvalidBalanceDelta(message)) => {
				ProviderCompleteJobResponse::InvalidBalanceDelta { message }
			}

			Err(Error::AlreadyFailed) => ProviderCompleteJobResponse::AlreadyFailed,

			Err(Error::AlreadyCompleted { completed_at_sync }) => {
				ProviderCompleteJobResponse::AlreadyCompleted { completed_at_sync }
			}
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ProviderCompleteJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
