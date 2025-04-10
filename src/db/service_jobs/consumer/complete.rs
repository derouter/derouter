use rusqlite::{Connection, params};

use crate::{
	db::{
		ConnectionLike,
		service_jobs::{
			JobHashingPayload, get::find_by_job_id, hash_job, validate_balance_delta,
		},
	},
	dto::JobRecord,
};

pub enum ConsumerCompleteJobError {
	InvalidJobId,
	ConsumerPeerIdMismatch,

	AlreadyFailed {
		reason: String,
		reason_class: Option<i64>,
	},

	AlreadyCompleted,
	InvalidBalanceDelta,
}

/// On the Consumer side, mark a synced job as completed, and sign it.
/// Neither `balance_delta` nor `public_payload` nor `completed_at_sync`
/// are validated in business sense (a consumer module should do it).
#[allow(clippy::too_many_arguments)]
pub fn consumer_complete_job<SignFn: Fn(&Vec<u8>) -> Vec<u8>>(
	conn: &mut Connection,
	consumer_peer_id: libp2p::PeerId,
	provider_peer_id: libp2p::PeerId,
	provider_job_id: &str,
	balance_delta: Option<&str>,
	public_payload: &str,
	private_payload: Option<&str>,
	completed_at_sync: i64,
	sign_fn: SignFn,
) -> Result<JobRecord, ConsumerCompleteJobError> {
	let tx = conn.transaction().unwrap();

	let job = find_by_job_id(
		ConnectionLike::Transaction(&tx),
		provider_peer_id,
		provider_job_id,
	);

	let mut job = match job {
		Some(job) => {
			if job.consumer_peer_id != consumer_peer_id {
				return Err(ConsumerCompleteJobError::ConsumerPeerIdMismatch);
			} else if let Some(reason) = job.reason {
				return Err(ConsumerCompleteJobError::AlreadyFailed {
					reason,
					reason_class: job.reason_class,
				});
			} else if job.completed_at_local.is_some() {
				return Err(ConsumerCompleteJobError::AlreadyCompleted);
			} else if let Some(balance_delta) = &balance_delta {
				if validate_balance_delta(balance_delta, job.currency).is_err() {
					return Err(ConsumerCompleteJobError::InvalidBalanceDelta);
				}
			}

			job
		}

		None => return Err(ConsumerCompleteJobError::InvalidJobId),
	};

	let balance_delta = balance_delta.map(hex::encode);

	let job_hashing_payload = JobHashingPayload {
		consumer_peer_id: job.consumer_peer_id,
		provider_peer_id: job.provider_peer_id,
		protocol_id: &job.offer_protocol_id,
		offer_payload: &job.offer_payload,
		currency: job.currency.code(),
		provider_job_id: &job.provider_job_id,
		job_public_payload: public_payload,
		job_completed_at_sync: completed_at_sync,
		job_created_at_sync: job.created_at_sync,
		balance_delta: balance_delta.as_deref(),
	};

	let job_hash = hash_job(&job_hashing_payload);
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
			job.job_rowid,
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

	job.balance_delta = balance_delta;
	job.public_payload = Some(public_payload.to_string());

	if let Some(private_payload) = private_payload {
		job.private_payload = Some(private_payload.to_string());
	}

	job.completed_at_local = Some(completed_at_local);
	job.completed_at_sync = Some(completed_at_sync);

	Ok(job)
}
