use rusqlite::{Connection, OptionalExtension, params};

use crate::{
	db::{
		ConnectionLike,
		service_jobs::{get::find_by_rowid, validate_balance_delta},
	},
	dto::{Currency, JobRecord},
};

pub enum ProviderCompleteJobError {
	JobNotFound,

	AlreadyCompleted {
		completed_at_sync: i64,
	},

	AlreadyFailed {
		reason: String,
		reason_class: Option<i64>,
	},

	InvalidBalanceDelta,
}

/// Mark a job as completed, locally.
pub fn provider_complete_job(
	conn: &mut Connection,
	provider_peer_id: libp2p::PeerId,
	provider_job_id: &str,
	balance_delta: Option<&str>,
	private_payload: Option<&str>,
	public_payload: &str,
) -> Result<JobRecord, ProviderCompleteJobError> {
	let tx = conn.transaction().unwrap();

	let job_rowid = {
		struct JobRow {
			rowid: i64,
			completed_at_sync: Option<i64>,
			currency: Currency,
			reason: Option<String>,
			reason_class: Option<i64>,
		}

		let job = tx
			.query_row(
				r#"
          SELECT
						service_jobs.rowid,             -- #0
						service_jobs.completed_at_sync, -- #1
						service_jobs.currency,          -- #2
						service_jobs.reason,            -- #3
						service_jobs.reason_class       -- #4
          FROM
						service_jobs
					JOIN offer_snapshots
						ON offer_snapshots.rowid = service_jobs.offer_snapshot_rowid
          WHERE
						offer_snapshots.provider_peer_id = ?1 AND
						service_jobs.provider_job_id = ?2
        "#,
				params![provider_peer_id.to_base58(), provider_job_id],
				|row| {
					Ok(JobRow {
						rowid: row.get(0)?,
						completed_at_sync: row.get(1)?,
						currency: Currency::try_from(row.get_ref(2)?.as_i64()?).unwrap(),
						reason: row.get(3)?,
						reason_class: row.get(4)?,
					})
				},
			)
			.optional()
			.unwrap();

		let job = match job {
			Some(job) => {
				if let Some(reason) = job.reason {
					return Err(ProviderCompleteJobError::AlreadyFailed {
						reason,
						reason_class: job.reason_class,
					});
				} else if let Some(completed_at_sync) = job.completed_at_sync {
					return Err(ProviderCompleteJobError::AlreadyCompleted {
						completed_at_sync,
					});
				} else if let Some(balance_delta) = &balance_delta {
					if validate_balance_delta(balance_delta, job.currency).is_err() {
						return Err(ProviderCompleteJobError::InvalidBalanceDelta);
					}
				}

				job
			}

			None => {
				return Err(ProviderCompleteJobError::JobNotFound);
			}
		};

		let completed_at_sync = chrono::Utc::now().timestamp();

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

						completed_at_sync = ?5
					WHERE
						rowid = ?1
				"#,
			)
			.unwrap();

		update_job_stmt
			.execute(params![
				job.rowid,
				balance_delta,
				public_payload,
				private_payload,
				completed_at_sync
			])
			.unwrap();

		job.rowid
	};

	let job_record =
		find_by_rowid(ConnectionLike::Transaction(&tx), job_rowid).unwrap();

	tx.commit().unwrap();

	Ok(job_record)
}
