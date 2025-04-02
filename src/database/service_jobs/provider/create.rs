use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::{database::service_jobs::get::get_job, dto::JobRecord};

pub enum ProviderCreateJobError {
	ConnectionNotFound,
}

/// Create a new service job, Provider-side.
pub fn provider_create_job(
	database: &mut Connection,
	connection_rowid: i64,
	private_payload: Option<String>,
) -> Result<JobRecord, ProviderCreateJobError> {
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
			return Err(ProviderCreateJobError::ConnectionNotFound);
		}

		let mut insert_job_stmt = tx
			.prepare_cached(
				r#"
          INSERT
          INTO service_jobs (
            provider_job_id,
            connection_rowid,
						private_payload,
						created_at_sync
          )
          VALUES (?1, ?2, ?3, ?4)
        "#,
			)
			.unwrap();

		let provider_job_id = Uuid::now_v7().to_string();
		let created_at_sync = chrono::Utc::now().timestamp();

		insert_job_stmt
			.execute(params![
				provider_job_id,
				connection_rowid,
				private_payload,
				created_at_sync
			])
			.unwrap();

		tx.last_insert_rowid()
	};

	let job_record = get_job(&tx, job_rowid).unwrap();
	tx.commit().unwrap();

	Ok(job_record)
}
