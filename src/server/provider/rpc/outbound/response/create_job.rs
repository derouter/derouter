use serde::Serialize;

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum CreateJobResponse {
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
