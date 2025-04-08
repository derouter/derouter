use rusqlite::{Connection, OptionalExtension, params};

use crate::{
	db::{
		service_connections::Currency,
		service_jobs::{get::get_job, validate_balance_delta},
	},
	dto::JobRecord,
};

pub enum ProviderCompleteJobError {
	InvalidJobId,
	AlreadyCompleted { completed_at_sync: i64 },
	AlreadyFailed,
	InvalidBalanceDelta(String),
}

/// Mark a job as completed, locally.
pub fn provider_complete_job(
	conn: &mut Connection,
	job_rowid: i64,
	balance_delta: Option<String>,
	private_payload: Option<String>,
	public_payload: String,
) -> Result<JobRecord, ProviderCompleteJobError> {
	let tx = conn.transaction().unwrap();

	{
		struct JobRow {
			completed_at_sync: Option<i64>,
			currency: Currency,
			reason: Option<String>,
		}

		let job = tx
			.query_row(
				r#"
          SELECT
						service_jobs.completed_at_sync, -- #0
						service_connections.currency,   -- #1
						service_jobs.reason             -- #2
          FROM service_jobs
					JOIN service_connections
						ON service_connections.rowid = service_jobs.connection_rowid
          WHERE service_jobs.rowid = ?1
        "#,
				[job_rowid],
				|row| {
					Ok(JobRow {
						completed_at_sync: row.get(0)?,
						currency: Currency::try_from(row.get_ref(1)?.as_i64()?).unwrap(),
						reason: row.get(2)?,
					})
				},
			)
			.optional()
			.unwrap();

		match job {
			Some(job) => {
				if job.reason.is_some() {
					return Err(ProviderCompleteJobError::AlreadyFailed);
				} else if let Some(completed_at_sync) = job.completed_at_sync {
					return Err(ProviderCompleteJobError::AlreadyCompleted {
						completed_at_sync,
					});
				} else if let Some(balance_delta) = &balance_delta {
					if let Some(err) = validate_balance_delta(balance_delta, job.currency)
					{
						return Err(ProviderCompleteJobError::InvalidBalanceDelta(err));
					}
				}
			}

			None => {
				return Err(ProviderCompleteJobError::InvalidJobId);
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
				job_rowid,
				balance_delta,
				public_payload,
				private_payload,
				completed_at_sync
			])
			.unwrap();

		completed_at_sync
	};

	let job_record = get_job(&tx, job_rowid).unwrap();
	tx.commit().unwrap();

	Ok(job_record)
}
