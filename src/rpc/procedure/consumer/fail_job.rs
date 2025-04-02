use serde::{Deserialize, Serialize};

use crate::{
	database::service_jobs::fail::fail_job,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
};

/// Mark a previously [created](CreateJob) job as failed.
/// May also be called after [SyncJob].
#[derive(Deserialize, Debug)]
pub struct ConsumerFailJobRequest {
	/// Job ID returned by [`CreateJob`].
	database_job_id: i64,

	/// The failure reason.
	reason: String,

	/// Protocol-specific faiulre class.
	reason_class: Option<i64>,

	/// Private, local-stored payload.
	/// Would override if already set.
	private_payload: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConsumerFailJobResponse {
	Ok,
	InvalidJobId,
	AlreadyCompleted,
	AlreadyFailed,
}

impl Connection {
	pub async fn handle_consumer_fail_job_request(
		&self,
		request_id: u32,
		request_data: ConsumerFailJobRequest,
	) {
		type Error = crate::database::service_jobs::fail::FailJobError;

		let response = match fail_job(
			&mut *self.state.database.lock().await,
			request_data.database_job_id,
			request_data.reason,
			request_data.reason_class,
			request_data.private_payload,
		) {
			Ok(_) => ConsumerFailJobResponse::Ok,
			Err(Error::InvalidJobId) => ConsumerFailJobResponse::InvalidJobId,
			Err(Error::AlreadyCompleted) => ConsumerFailJobResponse::AlreadyCompleted,
			Err(Error::AlreadyFailed) => ConsumerFailJobResponse::AlreadyFailed,
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ConsumerFailJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
