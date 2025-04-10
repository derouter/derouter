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

/// Mark a job as failed.
#[derive(Deserialize, Debug)]
pub struct FailJobRequest {
	provider_peer_id: libp2p::PeerId,
	provider_job_id: String,

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
pub enum FailJobResponse {
	Ok,

	JobNotFound,
	AlreadyCompleted,

	AlreadyFailed {
		reason: String,
		reason_class: Option<i64>,
	},
}

impl Connection {
	pub async fn handle_fail_job_request(
		&self,
		request_id: u32,
		request_data: FailJobRequest,
	) {
		type Error = crate::db::service_jobs::fail::FailJobError;

		let response = match fail_job(
			&mut *self.state.db.lock().await,
			request_data.provider_peer_id,
			&request_data.provider_job_id,
			&request_data.reason,
			request_data.reason_class,
			request_data.private_payload.as_deref(),
		) {
			Ok(_) => FailJobResponse::Ok,
			Err(Error::InvalidJobId) => FailJobResponse::JobNotFound,
			Err(Error::AlreadyCompleted) => FailJobResponse::AlreadyCompleted,

			Err(Error::AlreadyFailed {
				reason,
				reason_class,
			}) => FailJobResponse::AlreadyFailed {
				reason,
				reason_class,
			},
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::FailJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
