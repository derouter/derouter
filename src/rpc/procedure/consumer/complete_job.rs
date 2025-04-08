use futures::executor::block_on;
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
};

/// Mark a previously [synchronized](SyncJob) job as completed.
/// Shall call [ConfirmJobCompletion] afterwards.
#[derive(Deserialize, derive_more::Debug)]
pub struct ConsumerCompleteJobRequest {
	/// Job ID returned by [`CreateJob`].
	database_job_id: i64,

	/// Publicly-available job completion timestamp, told by Provider.
	completed_at_sync: i64,

	/// Publicly-available balance delta in [Currency]-specific encoding.
	balance_delta: Option<String>,

	/// Publicly-available job payload.
	#[debug(skip)]
	public_payload: String,

	/// Private, local-stored payload.
	/// Would override if already set.
	#[debug(skip)]
	private_payload: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConsumerCompleteJobResponse {
	Ok,

	/// Either the local P2P node is not running,
	/// or its peer ID mismatches the job's.
	InvalidConsumerPeerId {
		message: String,
	},

	InvalidJobId,
	NotSyncedYet,
	AlreadyCompleted,

	InvalidBalanceDelta {
		message: String,
	},
}

impl Connection {
	pub async fn handle_consumer_complete_job_request(
		&self,
		request_id: u32,
		request_data: ConsumerCompleteJobRequest,
	) {
		type Error =
			crate::db::service_jobs::consumer::complete::ConsumerCompleteJobError;

		let p2p_lock = self.state.p2p.lock().await;
		let public_key = p2p_lock.keypair.public();
		drop(p2p_lock);

		let response = {
			match consumer_complete_job(
				&mut *self.state.db.lock().await,
				public_key.to_peer_id().to_base58(),
				request_data.database_job_id,
				request_data.balance_delta,
				request_data.public_payload,
				request_data.private_payload,
				request_data.completed_at_sync,
				|job_hash| {
					// ADHOC: Functions w/ `rusqlite::Connection` may not be `async`.
					block_on(async {
						self.state.p2p.lock().await.keypair.sign(job_hash).unwrap()
					})
				},
			)
			.await
			{
				Ok(job_record) => {
					let _ = self
						.state
						.rpc
						.lock()
						.await
						.event_tx
						.send(RpcEvent::JobUpdated(Box::new(job_record)));

					ConsumerCompleteJobResponse::Ok
				}

				Err(Error::InvalidJobId) => ConsumerCompleteJobResponse::InvalidJobId,

				Err(Error::ConsumerPeerIdMismatch) => {
					ConsumerCompleteJobResponse::InvalidConsumerPeerId {
						message: "Local peer ID doesn't match the job's".to_string(),
					}
				}

				Err(Error::NotSyncedYet) => ConsumerCompleteJobResponse::NotSyncedYet,

				Err(Error::AlreadyCompleted) => {
					ConsumerCompleteJobResponse::AlreadyCompleted
				}

				Err(Error::InvalidBalanceDelta { message }) => {
					ConsumerCompleteJobResponse::InvalidBalanceDelta { message }
				}
			}
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ConsumerCompleteJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
