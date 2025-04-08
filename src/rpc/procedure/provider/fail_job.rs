use serde::{Deserialize, Serialize};

use crate::{
	db::service_jobs::fail::fail_job,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
};

/// Mark a job as failed, locally.
#[derive(Deserialize, derive_more::Debug)]
pub struct ProviderFailJobRequest {
	/// Local job ID.
	database_job_id: i64,

	/// Reason for the failure.
	reason: String,

	/// Protocol-specific failure class, if any.
	reason_class: Option<i64>,

	/// Private job payload, stored locally.
	/// Would override if already set.
	#[debug(skip)]
	private_payload: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ProviderFailJobResponse {
	Ok,
	InvalidJobId,
	AlreadyCompleted,
	AlreadyFailed,
}

impl Connection {
	pub async fn handle_provider_fail_job_request(
		&self,
		request_id: u32,
		request_data: ProviderFailJobRequest,
	) {
		type Error = crate::db::service_jobs::fail::FailJobError;

		let response = match fail_job(
			&mut *self.state.db.lock().await,
			request_data.database_job_id,
			request_data.reason,
			request_data.reason_class,
			request_data.private_payload,
		) {
			Ok(_) => ProviderFailJobResponse::Ok,
			Err(Error::InvalidJobId) => ProviderFailJobResponse::InvalidJobId,
			Err(Error::AlreadyCompleted) => ProviderFailJobResponse::AlreadyCompleted,
			Err(Error::AlreadyFailed) => ProviderFailJobResponse::AlreadyFailed,
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ProviderFailJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
