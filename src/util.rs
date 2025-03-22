use std::sync::Arc;

use tokio::sync::Mutex;

pub mod cbor;
pub mod yamux;

pub type ArcMutex<T> = Arc<Mutex<T>>;

pub fn to_arc_mutex<T>(value: T) -> ArcMutex<T> {
	Arc::new(Mutex::new(value))
}
