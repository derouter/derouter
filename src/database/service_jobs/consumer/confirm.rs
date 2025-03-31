use rusqlite::{Connection, OptionalExtension};

pub enum ConsumerConfirmJobResult {
	Ok,
	InvalidJobId,
	NotSignedYet,
	AlreadyConfirmed,
}

/// Mark a previosly completed job as confirmed
/// after an actual by-wire confirmation took place.
pub fn consumer_confirm_job(
	database: &mut Connection,
	job_rowid: i64,
) -> ConsumerConfirmJobResult {
	let tx = database.transaction().unwrap();

	struct JobRow {
		consumer_signature: Option<Vec<u8>>,
		signature_confirmed_at_local: Option<chrono::DateTime<chrono::Utc>>,
	}

	let job = tx
		.query_row(
			r#"
				SELECT
					consumer_signature,          -- #0
					signature_confirmed_at_local -- #1
				FROM service_jobs
				WHERE
					rowid = ?1
			"#,
			[job_rowid],
			|row| {
				Ok(JobRow {
					consumer_signature: row.get(0)?,
					signature_confirmed_at_local: row.get(1)?,
				})
			},
		)
		.optional()
		.unwrap();

	match job {
		Some(job) => {
			if job.signature_confirmed_at_local.is_some() {
				return ConsumerConfirmJobResult::AlreadyConfirmed;
			} else if job.consumer_signature.is_none() {
				return ConsumerConfirmJobResult::NotSignedYet;
			}
		}

		None => return ConsumerConfirmJobResult::InvalidJobId,
	};

	let mut update_job_stmt = tx
		.prepare_cached(
			r#"
				UPDATE
					service_jobs
				SET
					signature_confirmed_at_local = CURRENT_TIMESTAMP,
					confirmation_error = NULL
				WHERE
					rowid = ?1
			"#,
		)
		.unwrap();

	update_job_stmt.execute([job_rowid]).unwrap();

	drop(update_job_stmt);
	tx.commit().unwrap();

	ConsumerConfirmJobResult::Ok
}
