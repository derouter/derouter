use serde::{Deserialize, Serialize};

use crate::{
	database::service_jobs::provider::create::provider_create_job,
	rpc::{
		connection::Connection,
		procedure::{
			OutboundFrame, OutboundResponseFrame, OutboundResponseFrameData,
		},
	},
	state::RpcEvent,
};

/// Create a new incomplete service job
/// associated with given connection, locally.
#[derive(Deserialize, derive_more::Debug)]
pub struct ProviderCreateJobRequest {
	/// Local connection ID.
	connection_id: i64,

	/// Private job payload, stored locally.
	#[debug(skip)]
	private_payload: Option<String>,
}

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum ProviderCreateJobResponse {
	Ok {
		/// Used to reference the job by a local module.
		database_job_id: i64,

		/// Used by the Consumer to identify the job.
		provider_job_id: String,

		/// Timestamp in Provider's clock for synchronization.
		created_at_sync: i64,
	},

	InvalidConnectionId,
}

impl Connection {
	pub async fn handle_provider_create_job_request(
		&self,
		request_id: u32,
		request_data: ProviderCreateJobRequest,
	) {
		type Error =
			crate::database::service_jobs::provider::create::ProviderCreateJobError;

		let response = match provider_create_job(
			&mut *self.state.database.lock().await,
			request_data.connection_id,
			request_data.private_payload,
		) {
			Ok(job_record) => {
				let database_job_id = job_record.job_rowid;
				let provider_job_id = job_record.provider_job_id.clone().unwrap();
				let created_at_sync = job_record.created_at_sync.unwrap();

				let _ = self
					.state
					.rpc
					.lock()
					.await
					.event_tx
					.send(RpcEvent::JobUpdated(Box::new(job_record)));

				ProviderCreateJobResponse::Ok {
					database_job_id,
					provider_job_id,
					created_at_sync,
				}
			}

			Err(Error::ConnectionNotFound) => {
				ProviderCreateJobResponse::InvalidConnectionId
			}
		};

		let outbound_frame = OutboundFrame::Response(OutboundResponseFrame {
			id: request_id,
			data: OutboundResponseFrameData::ProviderCreateJob(response),
		});

		let _ = self.outbound_tx.send(outbound_frame).await;
	}
}
