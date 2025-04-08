use serde::{Deserialize, Serialize};

use crate::{
	db::service_jobs::consumer::create::consumer_create_job,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	state::RpcEvent,
};

/// Create a new "uninitialized" service job locally,
/// independent from the Provider. An uninitialized job
/// may be used to record pre-response Provider errors,
/// when its `provider_job_id` is not yet known.
/// Shall call [SyncJob] before [CompleteJob].
#[derive(Deserialize, derive_more::Debug)]
pub struct ConsumerCreateJobRequest {
	/// The associated connection ID.
	connection_id: i64,

	/// Private job payload, stored locally.
	#[debug(skip)]
	private_payload: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ConsumerCreateJobResponse {
	Ok {
		/// Local job ID.
		database_job_id: i64,
	},

	/// Connection with provided ID not found in the DB.
	ConnectionNotFound,

	/// A job with the same `provider_job_id` already exists in the DB.
	/// Likely a malformed provider responding with a repeated job ID.
	// BUG: Implement.
	#[allow(dead_code)]
	ProviderJobIdUniqueness,
}

impl Connection {
	pub async fn handle_consumer_create_job_request(
		&self,
		request_id: u32,
		request_data: ConsumerCreateJobRequest,
	) {
		type Error =
			crate::db::service_jobs::consumer::create::ConsumerCreateJobError;

		let response = match consumer_create_job(
			&mut *self.state.db.lock().await,
			request_data.connection_id,
			request_data.private_payload,
		) {
			Ok(job_record) => {
				let database_job_id = job_record.job_rowid;

				let _ = self
					.state
					.rpc
					.lock()
					.await
					.event_tx
					.send(RpcEvent::JobUpdated(Box::new(job_record)));

				ConsumerCreateJobResponse::Ok { database_job_id }
			}

			Err(Error::ConnectionNotFound) => {
				ConsumerCreateJobResponse::ConnectionNotFound
			}
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ConsumerCreateJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
