use std::sync::{Arc, LazyLock};

use chrono::TimeZone;
use tokio::sync::Mutex;

pub mod cbor;
pub mod yamux;

pub static TIME_ZERO: LazyLock<chrono::DateTime<chrono::Utc>> =
	LazyLock::new(|| chrono::Utc.timestamp_opt(0, 0).unwrap());

pub type ArcMutex<T> = Arc<Mutex<T>>;

pub fn to_arc<T>(value: T) -> Arc<T> {
	Arc::new(value)
}

pub fn to_arc_mutex<T>(value: T) -> ArcMutex<T> {
	to_arc(Mutex::new(value))
}
