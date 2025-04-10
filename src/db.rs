use std::{path::Path, rc::Rc, sync::LazyLock};

use cleanup::cleanup;
use include_dir::{Dir, include_dir};
use rusqlite::{CachedStatement, Connection, ToSql, Transaction, types::Value};
use rusqlite_migration::Migrations;

mod cleanup;
pub mod offers;
pub mod providers;
pub mod service_jobs;

pub use service_jobs::query_unconfirmed_jobs;

static DB_MIGRATIONS_DIR: Dir =
	include_dir!("$CARGO_MANIFEST_DIR/src/db/migrations");

static DB_MIGRATIONS: LazyLock<Migrations<'static>> =
	LazyLock::new(|| Migrations::from_directory(&DB_MIGRATIONS_DIR).unwrap());

pub fn open(path: &Path) -> eyre::Result<rusqlite::Connection> {
	log::debug!("Opening SQLite connection at {}", path.display());

	let mut conn = rusqlite::Connection::open(path)?;
	rusqlite::vtab::array::load_module(&conn)?;
	DB_MIGRATIONS.to_latest(&mut conn)?;
	cleanup(&conn, true);

	Ok(conn)
}

#[test]
fn db_migrations_test() {
	DB_MIGRATIONS.validate().unwrap();
}

// See https://github.com/rusqlite/rusqlite/issues/345#issuecomment-1694194547.
// See https://docs.rs/rusqlite/latest/rusqlite/vtab/array/index.html.
#[derive(Debug)]
pub enum Param {
	Single(Value),
	Array(Rc<Vec<Value>>),
}

impl ToSql for Param {
	fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
		match self {
			Param::Single(value) => value.to_sql(),
			Param::Array(values) => values.to_sql(),
		}
	}
}

pub enum ConnectionLike<'conn> {
	Connection(&'conn Connection),
	Transaction(&'conn Transaction<'conn>),
}

impl ConnectionLike<'_> {
	pub fn prepare_cached(
		&self,
		sql: &str,
	) -> Result<CachedStatement<'_>, rusqlite::Error> {
		match self {
			Self::Connection(connection) => connection.prepare_cached(sql),
			Self::Transaction(transaction) => transaction.prepare_cached(sql),
		}
	}
}
