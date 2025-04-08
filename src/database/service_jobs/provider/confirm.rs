use rusqlite::{Connection, OptionalExtension, params};

use crate::{
	database::{
		service_connections::Currency,
		service_jobs::{JobHashingPayload, hash_job},
	},
	dto::JobRecord,
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
	database: &mut Connection,
	consumer_peer_id: &libp2p::PeerId,
	provider_peer_id: &libp2p::PeerId,
	provider_job_id: &String,
	job_hash: &Vec<u8>,
	consumer_public_key: &[u8],
	consumer_signature: &Vec<u8>,
) -> Result<JobRecord, ProviderConfirmJobError> {
	let tx = database.transaction().unwrap();

	struct JobRow {
		reason: Option<String>,
		completed_at_sync: Option<i64>,
		signature_confirmed_at_local: Option<chrono::DateTime<chrono::Utc>>,
		currency: Currency,
		consumer_peer_id: String,
		provider_peer_id: String,
		protocol_id: String,
		offer_protocol_payload: String,
		public_payload: String,
		balance_delta: Option<String>,
		rowid: i64,
		created_at_sync: Option<i64>,
		completed_at_local: Option<chrono::DateTime<chrono::Utc>>,
		created_at_local: chrono::DateTime<chrono::Utc>,
		private_payload: Option<String>,
		offer_snapshot_rowid: i64,
		provider_job_id: Option<String>,
	}

	let job = tx
		.query_row(
			r#"
				SELECT
					service_jobs.reason,                       -- #0
					service_jobs.completed_at_sync,            -- #1
					service_jobs.signature_confirmed_at_local, -- #2
					service_connections.currency,              -- #3
					service_connections.consumer_peer_id,      -- #4
					offer_snapshots.provider_peer_id,          -- #5
					offer_snapshots.protocol_id,               -- #6
					offer_snapshots.protocol_payload,          -- #7
					service_jobs.public_payload,               -- #8
					service_jobs.balance_delta,                -- #9
					service_jobs.rowid,                        -- #10
					service_jobs.created_at_sync,              -- #11
					service_jobs.completed_at_local,           -- #12
					service_jobs.created_at_local,             -- #13
					service_jobs.private_payload,              -- #14
					offer_snapshots.rowid,                     -- #15
					service_jobs.provider_job_id               -- #16
				FROM service_jobs
				JOIN service_connections
					ON service_connections.rowid = service_jobs.connection_rowid
				JOIN offer_snapshots
					ON offer_snapshots.rowid = service_connections.offer_snapshot_rowid
				WHERE
					service_jobs.provider_job_id = ?1 AND
					offer_snapshots.provider_peer_id = ?2
			"#,
			[provider_job_id, &provider_peer_id.to_base58()],
			|row| {
				Ok(JobRow {
					reason: row.get(0)?,
					completed_at_sync: row.get(1)?,
					signature_confirmed_at_local: row.get(2)?,
					currency: Currency::try_from(row.get_ref(3)?.as_i64()?).unwrap(),
					consumer_peer_id: row.get(4)?,
					provider_peer_id: row.get(5)?,
					protocol_id: row.get(6)?,
					offer_protocol_payload: row.get(7)?,
					public_payload: row.get(8)?,
					balance_delta: row.get(9)?,
					rowid: row.get(10)?,
					created_at_sync: row.get(11)?,
					completed_at_local: row.get(12)?,
					created_at_local: row.get(13)?,
					private_payload: row.get(14)?,
					offer_snapshot_rowid: row.get(15)?,
					provider_job_id: row.get(16)?,
				})
			},
		)
		.optional()
		.unwrap();

	let job = match job {
		Some(job) => {
			if job.consumer_peer_id != consumer_peer_id.to_base58() {
				return Err(ProviderConfirmJobError::ConsumerPeerIdMismatch);
			} else if job.signature_confirmed_at_local.is_some() {
				return Err(ProviderConfirmJobError::AlreadyConfirmed);
			} else if job.created_at_sync.is_none() || job.completed_at_sync.is_none()
			{
				return Err(ProviderConfirmJobError::NotCompletedYet);
			} else if job.reason.is_some() {
				return Err(ProviderConfirmJobError::AlreadyFailed);
			}

			job
		}

		None => return Err(ProviderConfirmJobError::JobNotFound),
	};

	let job_hashing_payload = JobHashingPayload {
		consumer_peer_id: job.consumer_peer_id,
		provider_peer_id: job.provider_peer_id.clone(),
		protocol_id: job.protocol_id.clone(),
		offer_payload: job.offer_protocol_payload,
		currency: job.currency.code().to_string(),
		provider_job_id: provider_job_id.clone(),
		job_public_payload: job.public_payload.clone(),
		job_completed_at_sync: job.completed_at_sync.unwrap(),
		balance_delta: job.balance_delta.as_ref().map(hex::encode),
		job_created_at_sync: job.created_at_sync.unwrap(),
	};

	let expected_hash = hash_job(&job_hashing_payload);

	if *job_hash != expected_hash {
		return Err(ProviderConfirmJobError::HashMismatch {
			job_rowid: job.rowid,
			expected_hash,
		});
	}

	match libp2p::identity::PublicKey::try_decode_protobuf(consumer_public_key) {
		Ok(public_key) => {
			if !public_key.verify(job_hash, consumer_signature) {
				return Err(ProviderConfirmJobError::SignatureVerificationFailed {
					job_rowid: job.rowid,
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
			job.rowid,
			&consumer_signature,
			signature_confirmed_at_local
		])
		.unwrap();

	drop(update_job_stmt);
	tx.commit().unwrap();

	Ok(JobRecord {
		job_rowid: job.rowid,
		provider_peer_id: provider_peer_id.to_base58(),
		consumer_peer_id: consumer_peer_id.to_base58(),
		offer_snapshot_rowid: job.offer_snapshot_rowid,
		offer_protocol_id: job.protocol_id,
		currency: job.currency,
		balance_delta: job.balance_delta,
		public_payload: Some(job.public_payload),
		private_payload: job.private_payload,
		reason: None,
		reason_class: None,
		created_at_local: job.created_at_local,
		created_at_sync: job.created_at_sync,
		completed_at_local: job.completed_at_local,
		completed_at_sync: job.completed_at_sync,
		signature_confirmed_at_local: Some(signature_confirmed_at_local),
		confirmation_error: None,
		provider_job_id: job.provider_job_id,
	})
}
