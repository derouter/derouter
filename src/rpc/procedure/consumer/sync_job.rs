use serde::{Deserialize, Serialize};

use crate::{
	db::service_jobs::consumer::sync::consumer_sync_job,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	state::RpcEvent,
};

/// Synchronize a previosuly [created](CreateJob) job with Provider's data.
/// Useful for long-running jobs, **mandatory** before a [CompleteJob] call.
#[derive(Deserialize, Debug)]
pub struct ConsumerSyncJobRequest {
	/// Job ID returned by [`CreateJob`].
	database_job_id: i64,

	/// Job ID as told by the Provider.
	provider_job_id: String,

	/// Optionally update the local private payload.
	private_payload: Option<String>,

	/// Job creation timestamp as told by the Provider, in Provider's clock.
	created_at_sync: i64,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConsumerSyncJobResponse {
	Ok,
	InvalidJobId,

	/// `provider_job_id` is already set for this job.
	AlreadySynced,

	/// A job with the same `provider_job_id` already exists in the DB.
	/// Likely a malformed provider responding with a repeated job ID.
	ProviderJobIdUniqueness,
}

impl Connection {
	pub async fn handle_consumer_sync_job_request(
		&self,
		request_id: u32,
		request_data: ConsumerSyncJobRequest,
	) {
		type Error = crate::db::service_jobs::consumer::sync::ConsumerSyncJobError;

		let response = match consumer_sync_job(
			&mut *self.state.database.lock().await,
			request_data.database_job_id,
			request_data.provider_job_id,
			request_data.private_payload,
			request_data.created_at_sync,
		) {
			Ok(job_record) => {
				let _ = self
					.state
					.rpc
					.lock()
					.await
					.event_tx
					.send(RpcEvent::JobUpdated(Box::new(job_record)));

				ConsumerSyncJobResponse::Ok
			}

			Err(Error::InvalidJobId) => ConsumerSyncJobResponse::InvalidJobId,
			Err(Error::AlreadySynced) => ConsumerSyncJobResponse::AlreadySynced,
			Err(Error::ProviderJobIdUniqueness) => {
				ConsumerSyncJobResponse::ProviderJobIdUniqueness
			}
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ConsumerSyncJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
