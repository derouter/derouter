use rusqlite::{Connection, OptionalExtension, params};

pub enum FailJobError {
	InvalidJobId,
	AlreadyCompleted,
	AlreadyFailed,
}

pub fn fail_job(
	database: &mut Connection,
	job_rowid: i64,
	reason: String,
	reason_class: Option<i64>,
	private_payload: Option<String>,
) -> Result<(), FailJobError> {
	let tx = database.transaction().unwrap();

	struct JobRow {
		reason: Option<String>,
		completed_at_local: Option<chrono::DateTime<chrono::Utc>>,
	}

	let job = tx
		.query_row(
			r#"
				SELECT
					reason,            -- #0
					completed_at_local -- #1
				FROM service_jobs
				WHERE rowid = ?1
			"#,
			[job_rowid],
			|row| {
				Ok(JobRow {
					reason: row.get(0)?,
					completed_at_local: row.get(1)?,
				})
			},
		)
		.optional()
		.unwrap();

	match job {
		Some(job) => {
			if job.reason.is_some() {
				return Err(FailJobError::AlreadyFailed);
			} else if job.completed_at_local.is_some() {
				return Err(FailJobError::AlreadyCompleted);
			}
		}

		None => return Err(FailJobError::InvalidJobId),
	};

	let mut update_job_stmt = tx
		.prepare_cached(
			r#"
				UPDATE
					service_jobs
				SET
					reason = ?2,
					reason_class = ?3,
					private_payload = ?4,
					completed_at_local = CURRENT_TIMESTAMP
				WHERE
					rowid = ?1
			"#,
		)
		.unwrap();

	update_job_stmt
		.execute(params![job_rowid, reason, reason_class, private_payload])
		.unwrap();

	drop(update_job_stmt);
	tx.commit().unwrap();

	Ok(())
}
