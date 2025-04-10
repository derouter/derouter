use serde::{Deserialize, Serialize};

use crate::{dto::Currency, util::format_secret_option};

/// Consumer asks Provider to create a new job.
#[derive(Serialize, Deserialize, derive_more::Debug, Clone)]
pub struct CreateJobRequest {
	pub protocol_id: String,
	pub offer_id: String,

	#[debug(skip)]
	pub offer_payload: String,

	pub currency: Currency,

	#[debug("{}", format_secret_option(job_args))]
	pub job_args: Option<String>,
}

/// Consumer queries a job.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GetJobRequest {
	pub provider_job_id: String,
}

/// Consumer sends a completed job signature.
#[derive(Serialize, Deserialize, derive_more::Debug, Clone)]
pub struct ConfirmJobRequst {
	pub provider_job_id: String,
	pub job_hash: Vec<u8>,

	#[debug(skip)]
	pub consumer_public_key: Vec<u8>,

	#[debug(skip)]
	pub consumer_signature: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Request {
	CreateJob(CreateJobRequest),
	GetJob(GetJobRequest),

	/// As a Consumer, confirm job completion with a signature.
	ConfirmJob(ConfirmJobRequst),
}

/// Response to [CreateJobRequest].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CreateJobResponse {
	Ok {
		provider_job_id: String,
		created_at_sync: i64,
	},

	OfferNotFound,
	OfferPayloadMismatch,
	InvalidJobArgs(String),

	Busy,
}

/// Response to [GetJobRequest].
#[derive(Serialize, Deserialize, derive_more::Debug, Clone)]
pub enum GetJobResponse {
	Ok {
		#[debug("{}", format_secret_option(public_payload))]
		public_payload: Option<String>,
		balance_delta: Option<String>,
		created_at_sync: i64,
		completed_at_sync: Option<i64>,
	},

	/// May also indicate authorization error.
	JobNotFound,
}

#[derive(Serialize, Deserialize, derive_more::Debug, Clone)]
pub enum ConfirmJobResponse {
	Ok,

	/// Provider claims to not being able to find the job locally.
	JobNotFound,

	/// Provider claims that the job has not been completed yet.
	NotCompletedYet,

	/// Provider claims that the job has already been marked as failed.
	AlreadyFailed,

	/// Provider expects another job hash.
	#[debug("{}", hex::encode(_0))]
	HashMismatch(Vec<u8>),

	/// Provider claims `consumer_public_key` to be invalid.
	InvalidPublicKey,

	/// Provider claims `consumer_signature` to be invalid.
	InvalidSignature,

	/// Provider tells us that the hash and signature is valid,
	/// but the job is already confirmed.
	AlreadyConfirmed,

	UnexpectedResponse,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[allow(clippy::enum_variant_names)]
pub enum Response {
	CreateJob(CreateJobResponse),
	GetJob(GetJobResponse),
	ConfirmJob(ConfirmJobResponse),
}
