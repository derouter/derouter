use rusqlite::{Connection, params};

use crate::db::{
	ConnectionLike,
	service_jobs::{JobHashingPayload, get::find_by_job_id, hash_job},
};

pub enum ProviderConfirmJobError {
	/// Job not found locally.
	JobNotFound,

	/// The stored job has another Consumer peer ID.
	ConsumerPeerIdMismatch,

	/// The job is not marked as completed yet.
	NotCompletedYet,

	/// The job hash is valid, but the it is already signed (not a error).
	AlreadyFailed,

	/// Job hash mismatch.
	HashMismatch {
		job_rowid: i64,
		expected_hash: Vec<u8>,
	},

	/// The job's signature is already confirmed.
	AlreadyConfirmed,

	/// Failed to decode the provided Consumer public key.
	PublicKeyDecodingFailed(libp2p::identity::DecodingError),

	/// Failed to verify the provided Consumer signature.
	SignatureVerificationFailed { job_rowid: i64 },
}

/// As a Provider, confirm the Consumer's signature
/// on a [previously completed](provider_complete_job) job.
pub fn provider_confirm_job(
	conn: &mut Connection,
	consumer_peer_id: libp2p::PeerId,
	provider_peer_id: libp2p::PeerId,
	provider_job_id: &str,
	job_hash: &Vec<u8>,
	consumer_public_key: &[u8],
	consumer_signature: &Vec<u8>,
) -> Result<(), ProviderConfirmJobError> {
	let tx = conn.transaction().unwrap();

	let job = find_by_job_id(
		ConnectionLike::Transaction(&tx),
		provider_peer_id,
		provider_job_id,
	);

	let job = match job {
		Some(job) => {
			if job.consumer_peer_id != consumer_peer_id {
				return Err(ProviderConfirmJobError::ConsumerPeerIdMismatch);
			} else if job.signature_confirmed_at_local.is_some() {
				return Err(ProviderConfirmJobError::AlreadyConfirmed);
			} else if job.completed_at_sync.is_none() {
				return Err(ProviderConfirmJobError::NotCompletedYet);
			} else if job.reason.is_some() {
				return Err(ProviderConfirmJobError::AlreadyFailed);
			}

			job
		}

		None => return Err(ProviderConfirmJobError::JobNotFound),
	};

	let balance_delta = job.balance_delta.as_ref().map(hex::encode);

	let job_hashing_payload = JobHashingPayload {
		consumer_peer_id: job.consumer_peer_id,
		provider_peer_id: job.provider_peer_id,
		protocol_id: &job.offer_protocol_id,
		offer_payload: &job.offer_payload,
		currency: job.currency.code(),
		provider_job_id,
		job_public_payload: &job.public_payload.unwrap(),
		job_completed_at_sync: job.completed_at_sync.unwrap(),
		balance_delta: balance_delta.as_deref(),
		job_created_at_sync: job.created_at_sync,
	};

	let expected_hash = hash_job(&job_hashing_payload);

	if *job_hash != expected_hash {
		return Err(ProviderConfirmJobError::HashMismatch {
			job_rowid: job.job_rowid,
			expected_hash,
		});
	}

	match libp2p::identity::PublicKey::try_decode_protobuf(consumer_public_key) {
		Ok(public_key) => {
			if !public_key.verify(job_hash, consumer_signature) {
				return Err(ProviderConfirmJobError::SignatureVerificationFailed {
					job_rowid: job.job_rowid,
				});
			}
		}

		Err(e) => {
			return Err(ProviderConfirmJobError::PublicKeyDecodingFailed(e));
		}
	}

	let signature_confirmed_at_local = chrono::Utc::now();

	let mut update_job_stmt = tx
		.prepare_cached(
			r#"
				UPDATE
					service_jobs
				SET
					consumer_signature = ?2,
					signature_confirmed_at_local = ?3,
					confirmation_error = NULL -- Clear the error.
				WHERE
					rowid = ?1
			"#,
		)
		.unwrap();

	update_job_stmt
		.execute(params![
			job.job_rowid,
			&consumer_signature,
			signature_confirmed_at_local
		])
		.unwrap();

	drop(update_job_stmt);
	tx.commit().unwrap();

	Ok(())
}
