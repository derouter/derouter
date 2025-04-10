use std::sync::{Arc, LazyLock, atomic::AtomicUsize};

use chrono::TimeZone;

pub mod cbor;
pub mod yamux;

#[cfg(debug_assertions)]
pub mod logged_mutex;

#[cfg(debug_assertions)]
pub mod logged_rw_lock;

#[cfg(not(debug_assertions))]
use tokio::sync::Mutex;

#[cfg(not(debug_assertions))]
use tokio::sync::RwLock;

#[cfg(debug_assertions)]
static LOCK_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub static TIME_ZERO: LazyLock<chrono::DateTime<chrono::Utc>> =
	LazyLock::new(|| chrono::Utc.timestamp_opt(0, 0).unwrap());

pub fn to_arc<T>(value: T) -> Arc<T> {
	Arc::new(value)
}

#[cfg(debug_assertions)]
pub type ArcMutex<T> = Arc<logged_mutex::LoggedMutex<T>>;

#[cfg(debug_assertions)]
pub fn to_arc_mutex<T>(value: T) -> ArcMutex<T> {
	to_arc(logged_mutex::LoggedMutex::new(value))
}

#[cfg(not(debug_assertions))]
pub type ArcMutex<T> = Arc<Mutex<T>>;

#[cfg(not(debug_assertions))]
pub fn to_arc_mutex<T>(value: T) -> ArcMutex<T> {
	to_arc(Mutex::new(value))
}

#[cfg(debug_assertions)]
pub type ArcRw<T> = Arc<logged_rw_lock::LoggedRwLock<T>>;

#[cfg(debug_assertions)]
pub fn to_arc_rw<T>(value: T) -> ArcRw<T> {
	to_arc(logged_rw_lock::LoggedRwLock::new(value))
}

#[cfg(not(debug_assertions))]
pub type ArcRw<T> = Arc<RwLock<T>>;

#[cfg(not(debug_assertions))]
pub fn to_arc_rw<T>(value: T) -> ArcRw<T> {
	to_arc(RwLock::new(value))
}

/// Returns `"Some(..)"` if some, `"None"` otherwise.
#[allow(dead_code)]
pub fn format_secret_option<T>(option: &Option<T>) -> &'static str {
	if option.is_some() { "Some(..)" } else { "None" }
}
