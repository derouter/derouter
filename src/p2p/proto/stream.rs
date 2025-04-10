use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JobConnectionHeadRequest {
	pub provider_job_id: String,
}

/// Written immediately to an outbound stream.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum HeadRequest {
	JobConnection(JobConnectionHeadRequest),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum JobConnectionHeadResponse {
	Ok,

	/// Job not found by `provider_job_id`.
	JobNotFound,

	/// Job is found, but its contents is not available anymore.
	JobExpired,

	/// Provider is temporarily busy.
	Busy,

	AnotherError(String),
}

/// Written to the stream in response to [HeadRequest].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum HeadResponse {
	JobConnection(JobConnectionHeadResponse),
}
