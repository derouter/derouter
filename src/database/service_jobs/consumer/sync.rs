use rusqlite::{Connection, OptionalExtension, params};

pub enum ConsumerUpdateJobResult {
	Ok,
	InvalidJobId,
	AlreadySynced,

	// BUG: Implement.
	#[allow(dead_code)]
	ProviderJobIdUniqueness,
}

/// Synchronize a previously created job with Provider's data.
pub fn consumer_sync_job(
	database: &mut Connection,
	job_rowid: i64,
	provider_job_id: String,
	private_payload: Option<String>,
	created_at_sync: i64,
) -> ConsumerUpdateJobResult {
	let tx = database.transaction().unwrap();

	struct JobRow {
		provider_job_id: Option<String>,
	}

	let job = tx
		.query_row(
			r#"
				SELECT provider_job_id
				FROM service_jobs
				WHERE rowid = ?1
			"#,
			[job_rowid],
			|row| {
				Ok(JobRow {
					provider_job_id: row.get(0)?,
				})
			},
		)
		.optional()
		.unwrap();

	match job {
		Some(job) => {
			if job.provider_job_id.is_some() {
				return ConsumerUpdateJobResult::AlreadySynced;
			}
		}

		None => return ConsumerUpdateJobResult::InvalidJobId,
	};

	let mut update_job_stmt = tx
		.prepare_cached(
			r#"
				UPDATE
					service_jobs
				SET
					provider_job_id = ?2,

					-- Only update `private_payload`
					-- if the argument is not NULL.
					private_payload = CASE ?3
						WHEN ?3 IS NOT NULL THEN ?3
						ELSE private_payload
						END,

					created_at_sync = ?4
				WHERE
					rowid = ?1
			"#,
		)
		.unwrap();

	update_job_stmt
		.execute(params![
			job_rowid,
			provider_job_id,
			private_payload,
			created_at_sync
		])
		.unwrap();

	drop(update_job_stmt);
	tx.commit().unwrap();

	ConsumerUpdateJobResult::Ok
}
