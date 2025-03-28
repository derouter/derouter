use rusqlite::Connection;

/// Cleanup the database from stale data.
pub fn cleanup(_database: &Connection, _initial: bool) {
	log::warn!("Database cleanup is not implemented yet");
}
