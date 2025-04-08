use std::sync::{Arc, LazyLock};

#[cfg(debug_assertions)]
use std::{
	any::type_name,
	ops::{Deref, DerefMut},
	sync::atomic::{AtomicUsize, Ordering},
};

use chrono::TimeZone;
use tokio::sync::Mutex;

pub mod cbor;
pub mod yamux;

pub static TIME_ZERO: LazyLock<chrono::DateTime<chrono::Utc>> =
	LazyLock::new(|| chrono::Utc.timestamp_opt(0, 0).unwrap());

#[cfg(debug_assertions)]
static LOCK_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(debug_assertions)]
#[allow(dead_code)]
pub struct LoggedMutexGuard<'a, T: ?Sized>(
	tokio::sync::MutexGuard<'a, T>,
	usize,
);

#[cfg(debug_assertions)]
impl<T: ?Sized> Drop for LoggedMutexGuard<'_, T> {
	fn drop(&mut self) {
		log::trace!("🔓 Dropped lock #{} for `{}`", self.1, type_name::<T>());
	}
}

#[cfg(debug_assertions)]
impl<T: ?Sized> Deref for LoggedMutexGuard<'_, T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		self.0.deref()
	}
}

#[cfg(debug_assertions)]
impl<T: ?Sized> DerefMut for LoggedMutexGuard<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		self.0.deref_mut()
	}
}

#[cfg(debug_assertions)]
#[derive(Debug)]
pub struct LoggedMutex<T>(Mutex<T>);

#[cfg(debug_assertions)]
impl<T> LoggedMutex<T> {
	pub fn new(value: T) -> Self {
		Self(Mutex::new(value))
	}

	#[track_caller]
	pub fn lock(&self) -> impl Future<Output = LoggedMutexGuard<'_, T>> {
		let counter = LOCK_COUNTER.fetch_add(1, Ordering::Relaxed);
		log::trace!(
			"⏳ Acquiring lock #{} for `{}` at {}...",
			counter,
			type_name::<T>(),
			std::panic::Location::caller()
		);

		async move {
			let lock = LoggedMutexGuard(self.0.lock().await, counter);
			log::trace!("🔒 Acquired lock #{} for `{}`", counter, type_name::<T>());
			lock
		}
	}
}

#[cfg(debug_assertions)]
pub type ArcMutex<T> = Arc<LoggedMutex<T>>;

#[cfg(not(debug_assertions))]
pub type ArcMutex<T> = Arc<Mutex<T>>;

pub fn to_arc<T>(value: T) -> Arc<T> {
	Arc::new(value)
}

#[cfg(debug_assertions)]
pub fn to_arc_mutex<T>(value: T) -> ArcMutex<T> {
	to_arc(LoggedMutex::new(value))
}

#[cfg(not(debug_assertions))]
pub fn to_arc_mutex<T>(value: T) -> ArcMutex<T> {
	to_arc(Mutex::new(value))
}
