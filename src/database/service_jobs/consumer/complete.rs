use rusqlite::{Connection, OptionalExtension, params};

use crate::{
	database::{
		service_connections::Currency,
		service_jobs::{JobHashingPayload, hash_job, validate_balance_delta},
	},
	dto::JobRecord,
};

pub enum ConsumerCompleteJobError {
	InvalidJobId,
	ConsumerPeerIdMismatch,
	NotSyncedYet,
	AlreadyCompleted,
	InvalidBalanceDelta { message: String },
}

/// On the Consumer side, mark a synced job as completed, and sign it.
/// Neither `balance_delta` nor `public_payload` nor `completed_at_sync`
/// are validated in business sense (a consumer module should do it).
#[allow(clippy::too_many_arguments)]
pub async fn consumer_complete_job<SignFn: Fn(&Vec<u8>) -> Vec<u8>>(
	database: &mut Connection,
	consumer_peer_id: String,
	job_rowid: i64,
	balance_delta: Option<String>,
	public_payload: String,
	private_payload: Option<String>,
	completed_at_sync: i64,
	sign_fn: SignFn,
) -> Result<JobRecord, ConsumerCompleteJobError> {
	let tx = database.transaction().unwrap();

	struct JobRow {
		provider_job_id: Option<String>,
		completed_at_local: Option<chrono::DateTime<chrono::Utc>>,
		created_at_sync: Option<i64>,
		currency: Currency,
		consumer_peer_id: String,
		provider_peer_id: String,
		protocol_id: String,
		offer_protocol_payload: String,
		created_at_local: chrono::DateTime<chrono::Utc>,
		offer_snapshot_rowid: i64,
		private_payload: Option<String>,
	}

	let job = tx
		.query_row(
			r#"
				SELECT
					service_jobs.provider_job_id,         -- #0
					service_jobs.completed_at_local,      -- #1
					service_connections.currency,         -- #2
					service_connections.consumer_peer_id, -- #3
					offer_snapshots.provider_peer_id,     -- #4
					offer_snapshots.protocol_id,          -- #5
					offer_snapshots.protocol_payload,     -- #6
					service_jobs.created_at_sync,         -- #7
					service_jobs.created_at_local,        -- #8
					offer_snapshots.rowid,                -- #9
					service_jobs.private_payload          -- #10
				FROM service_jobs
				JOIN service_connections
					ON service_connections.rowid = service_jobs.connection_rowid
				JOIN offer_snapshots
					ON offer_snapshots.rowid = service_connections.offer_snapshot_rowid
				WHERE
					service_jobs.rowid = ?1
			"#,
			[job_rowid],
			|row| {
				Ok(JobRow {
					provider_job_id: row.get(0)?,
					completed_at_local: row.get(1)?,
					currency: Currency::try_from(row.get_ref(2)?.as_i64()?).unwrap(),
					consumer_peer_id: row.get(3)?,
					provider_peer_id: row.get(4)?,
					protocol_id: row.get(5)?,
					offer_protocol_payload: row.get(6)?,
					created_at_sync: row.get(7)?,
					created_at_local: row.get(8)?,
					offer_snapshot_rowid: row.get(9)?,
					private_payload: row.get(10)?,
				})
			},
		)
		.optional()
		.unwrap();

	let job = match job {
		Some(job) => {
			if job.consumer_peer_id != consumer_peer_id {
				return Err(ConsumerCompleteJobError::ConsumerPeerIdMismatch);
			} else if job.provider_job_id.is_none() || job.created_at_sync.is_none() {
				return Err(ConsumerCompleteJobError::NotSyncedYet);
			} else if job.completed_at_local.is_some() {
				return Err(ConsumerCompleteJobError::AlreadyCompleted);
			} else if let Some(balance_delta) = &balance_delta {
				if let Some(message) =
					validate_balance_delta(balance_delta, job.currency)
				{
					return Err(ConsumerCompleteJobError::InvalidBalanceDelta {
						message,
					});
				}
			}

			job
		}

		None => return Err(ConsumerCompleteJobError::InvalidJobId),
	};

	let job_hashing_payload = JobHashingPayload {
		consumer_peer_id: job.consumer_peer_id,
		provider_peer_id: job.provider_peer_id.clone(),
		protocol_id: job.protocol_id.clone(),
		offer_payload: job.offer_protocol_payload,
		currency: job.currency.code().to_string(),
		provider_job_id: job.provider_job_id.clone().unwrap(),
		job_public_payload: public_payload.clone(),
		job_completed_at_sync: completed_at_sync,
		job_created_at_sync: job.created_at_sync.unwrap(),
		balance_delta: balance_delta.as_ref().map(hex::encode),
	};

	let job_hash = hash_job(job_hashing_payload);
	let consumer_signature = sign_fn(&job_hash);
	let completed_at_local = chrono::Utc::now();

	let mut update_job_stmt = tx
		.prepare_cached(
			r#"
				UPDATE
					service_jobs
				SET
					balance_delta = ?2,
					public_payload = ?3,

					-- Only update `private_payload`
					-- if the argument is not NULL.
					private_payload = CASE
						WHEN (?4 IS NOT NULL) THEN ?4
						ELSE private_payload
						END,

					completed_at_local = ?5,
					completed_at_sync = ?6,
					hash = ?7,
					consumer_signature = ?8
				WHERE
					rowid = ?1
			"#,
		)
		.unwrap();

	update_job_stmt
		.execute(params![
			job_rowid,
			&balance_delta,
			&public_payload,
			&private_payload,
			completed_at_local,
			completed_at_sync,
			&job_hash,
			&consumer_signature
		])
		.unwrap();

	drop(update_job_stmt);
	tx.commit().unwrap();

	Ok(JobRecord {
		job_rowid,
		provider_peer_id: job.provider_peer_id,
		consumer_peer_id,
		offer_snapshot_rowid: job.offer_snapshot_rowid,
		offer_protocol_id: job.protocol_id,
		currency: job.currency,
		balance_delta,
		public_payload: Some(public_payload),
		private_payload: private_payload.or(job.private_payload),
		reason: None,
		reason_class: None,
		created_at_local: job.created_at_local,
		created_at_sync: job.created_at_sync,
		completed_at_local: Some(completed_at_local),
		completed_at_sync: Some(completed_at_sync),
		signature_confirmed_at_local: None,
		confirmation_error: None,
		provider_job_id: job.provider_job_id,
	})
}
