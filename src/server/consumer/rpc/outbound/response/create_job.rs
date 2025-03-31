use serde::Serialize;

#[derive(Serialize, Debug)]
#[serde(tag = "tag", content = "content")]
pub enum CreateJobResponse {
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
