use rusqlite::{Connection, OptionalExtension, params};

pub enum FailJobError {
	InvalidJobId,
	AlreadyCompleted,

	AlreadyFailed {
		reason: String,
		reason_class: Option<i64>,
	},
}

pub fn fail_job(
	conn: &mut Connection,
	provider_peer_id: libp2p::PeerId,
	provider_job_id: &str,
	reason: &str,
	reason_class: Option<i64>,
	private_payload: Option<&str>,
) -> Result<(), FailJobError> {
	let tx = conn.transaction().unwrap();

	struct JobRow {
		rowid: i64,
		reason: Option<String>,
		reason_class: Option<i64>,
		completed_at_local: Option<chrono::DateTime<chrono::Utc>>,
	}

	let job = tx
		.query_row(
			r#"
				SELECT
					service_jobs.rowid,             -- #0
					service_jobs.reason,            -- #1
					service_jobs.reason_class,      -- #2
					service_jobs.completed_at_local -- #3
				FROM service_jobs
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
					reason: row.get(1)?,
					reason_class: row.get(2)?,
					completed_at_local: row.get(3)?,
				})
			},
		)
		.optional()
		.unwrap();

	let job = match job {
		Some(job) => {
			if let Some(reason) = job.reason {
				return Err(FailJobError::AlreadyFailed {
					reason,
					reason_class: job.reason_class,
				});
			} else if job.completed_at_local.is_some() {
				return Err(FailJobError::AlreadyCompleted);
			}

			job
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
		.execute(params![job.rowid, reason, reason_class, private_payload])
		.unwrap();

	drop(update_job_stmt);
	tx.commit().unwrap();

	Ok(())
}
