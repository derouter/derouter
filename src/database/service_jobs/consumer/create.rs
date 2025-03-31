use rusqlite::{Connection, OptionalExtension, params};

pub enum ConsumerCreateJobError {
	ConnectionNotFound,
}

/// Create an "uninitialzed" service job,
/// exclusively on Consumer side.
/// Returns `job_rowid`.
pub fn consumer_create_job(
	database: &mut Connection,
	connection_rowid: i64,
	private_payload: Option<String>,
) -> Result<i64, ConsumerCreateJobError> {
	let tx = database.transaction().unwrap();

	let job_rowid = {
		// OPTIMIZE: Handle SQLite error instead of querying.
		let conn_exists = tx
			.query_row(
				r#"
          SELECT 1
          FROM service_connections
          WHERE rowid = ?1
        "#,
				[connection_rowid],
				|_| Ok(()),
			)
			.optional()
			.unwrap();

		if conn_exists.is_none() {
			return Err(ConsumerCreateJobError::ConnectionNotFound);
		}

		let mut insert_job_stmt = tx
			.prepare_cached(
				r#"
          INSERT
          INTO service_jobs (
            connection_rowid,
						private_payload
          )
          VALUES (?1, ?2)
        "#,
			)
			.unwrap();

		insert_job_stmt
			.execute(params![connection_rowid, private_payload])
			.unwrap();

		tx.last_insert_rowid()
	};

	tx.commit().unwrap();
	Ok(job_rowid)
}
