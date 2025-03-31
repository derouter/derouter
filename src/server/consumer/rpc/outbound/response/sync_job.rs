use serde::Serialize;

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum SyncJobResponse {
	Ok,
	InvalidJobId,

	/// `provider_job_id` is already set for this job.
	AlreadySynced,

	/// A job with the same `provider_job_id` already exists in the DB.
	/// Likely a malformed provider responding with a repeated job ID.
	ProviderJobIdUniqueness,
}
