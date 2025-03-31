use rusqlite::{Connection, OptionalExtension};

pub enum SetJobConfirmationErrorResult {
	Ok,
	InvalidJobId,
	AlreadyConfirmed,
}

/// Set the job's `confirmation_error`.
pub fn set_job_confirmation_error(
	database: &mut Connection,
	job_rowid: i64,
	confirmation_error: &str,
) -> SetJobConfirmationErrorResult {
	let tx = database.transaction().unwrap();

	struct JobRow {
		signature_confirmed_at_local: Option<chrono::DateTime<chrono::Utc>>,
	}

	let job = tx
		.query_row(
			r#"
				SELECT
					signature_confirmed_at_local
				FROM service_jobs
				WHERE
					rowid = ?1
			"#,
			[job_rowid],
			|row| {
				Ok(JobRow {
					signature_confirmed_at_local: row.get(0)?,
				})
			},
		)
		.optional()
		.unwrap();

	match job {
		Some(job) => {
			if job.signature_confirmed_at_local.is_some() {
				return SetJobConfirmationErrorResult::AlreadyConfirmed;
			}
		}

		None => return SetJobConfirmationErrorResult::InvalidJobId,
	};

	let mut update_job_stmt = tx
		.prepare_cached(
			r#"
				UPDATE
					service_jobs
				SET
					confirmation_error = ?2
				WHERE
					rowid = ?1
			"#,
		)
		.unwrap();

	update_job_stmt.execute([confirmation_error]).unwrap();

	drop(update_job_stmt);
	tx.commit().unwrap();

	SetJobConfirmationErrorResult::Ok
}
