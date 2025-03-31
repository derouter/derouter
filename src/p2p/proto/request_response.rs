use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Request {
	/// As a Consumer, confirm job completion by sending a signature.
	ConfirmJobCompletion {
		provider_job_id: String,
		job_hash: Vec<u8>,
		consumer_public_key: Vec<u8>,
		consumer_signature: Vec<u8>,
	},
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ConfirmJobCompletionResponse {
	Ok,

	/// Matching job not found locally.
	JobNotFound,

	/// The job is not marked as completed yet.
	NotCompletedYet,

	/// The job has failed, therefore allows no signature.
	AlreadyFailed,

	/// Job hash mismatch.
	HashMismatch {
		expected: Vec<u8>,
	},

	/// Failed to decode the provided Consumer public key.
	PublicKeyDecodingFailed,

	/// Failed to verify the provided Consumer signature.
	SignatureVerificationFailed,

	/// The hash and signature is valid, but already confirmed.
	AlreadyConfirmed,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Response {
	ConfirmJobCompletion(ConfirmJobCompletionResponse),
}
