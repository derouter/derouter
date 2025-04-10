use serde::{Deserialize, Serialize};

use crate::{
	db::service_jobs::consumer::complete::consumer_complete_job,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	state::RpcEvent,
	util::format_secret_option,
};

/// Confirm job completion, and schedule a signature for sending to Provider.
#[derive(Deserialize, derive_more::Debug)]
pub struct ConsumerCompleteJobRequest {
	provider_peer_id: libp2p::PeerId,
	provider_job_id: String,

	/// Publicly-available job payload.
	#[debug(skip)]
	public_payload: String,

	/// Private, locally stored payload.
	/// Would overwrite if already set.
	#[debug("{}", format_secret_option(private_payload))]
	private_payload: Option<String>,

	/// Publicly-available balance delta in [Currency]-specific encoding.
	balance_delta: Option<String>,

	/// Publicly-available job completion timestamp, as told by the Provider.
	completed_at_sync: i64,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConsumerCompleteJobResponse {
	Ok,
	InvalidJobId,

	/// Current node's peer ID mismatches the job's.
	InvalidConsumerPeerId,

	AlreadyFailed {
		reason: String,
		reason_class: Option<i64>,
	},

	AlreadyCompleted,
	InvalidBalanceDelta,
}

impl Connection {
	pub async fn handle_consumer_complete_job_request(
		&self,
		request_id: u32,
		request_data: ConsumerCompleteJobRequest,
	) {
		type Error =
			crate::db::service_jobs::consumer::complete::ConsumerCompleteJobError;

		let keypair = self.state.p2p.lock().await.keypair.clone();

		let response = match consumer_complete_job(
			&mut *self.state.db.lock().await,
			keypair.public().to_peer_id(),
			request_data.provider_peer_id,
			&request_data.provider_job_id,
			request_data.balance_delta.as_deref(),
			&request_data.public_payload,
			request_data.private_payload.as_deref(),
			request_data.completed_at_sync,
			|job_hash| keypair.sign(job_hash).unwrap(),
		) {
			Ok(job_record) => {
				let _ = self
					.state
					.rpc
					.lock()
					.await
					.event_tx
					.send(RpcEvent::JobUpdated(Box::new(job_record)));

				self.state.consumer.job_completion_notify.notify_waiters();
				ConsumerCompleteJobResponse::Ok
			}

			Err(Error::InvalidJobId) => ConsumerCompleteJobResponse::InvalidJobId,

			Err(Error::ConsumerPeerIdMismatch) => {
				ConsumerCompleteJobResponse::InvalidConsumerPeerId
			}

			Err(Error::AlreadyFailed {
				reason,
				reason_class,
			}) => ConsumerCompleteJobResponse::AlreadyFailed {
				reason,
				reason_class,
			},

			Err(Error::AlreadyCompleted) => {
				ConsumerCompleteJobResponse::AlreadyCompleted
			}

			Err(Error::InvalidBalanceDelta) => {
				ConsumerCompleteJobResponse::InvalidBalanceDelta
			}
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ConsumerCompleteJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
