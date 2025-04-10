use std::{
	any::type_name,
	ops::{Deref, DerefMut},
	sync::atomic::Ordering,
};

use tokio::sync::Mutex;

use crate::util::LOCK_COUNTER;

#[allow(dead_code)]
pub struct LoggedMutexGuard<'a, T: ?Sized>(
	tokio::sync::MutexGuard<'a, T>,
	usize,
);

impl<T: ?Sized> Drop for LoggedMutexGuard<'_, T> {
	fn drop(&mut self) {
		log::trace!("🔓 Dropped lock #{} for `{}`", self.1, type_name::<T>());
	}
}

impl<T: ?Sized> Deref for LoggedMutexGuard<'_, T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		self.0.deref()
	}
}

impl<T: ?Sized> DerefMut for LoggedMutexGuard<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		self.0.deref_mut()
	}
}

#[derive(Debug)]
pub struct LoggedMutex<T>(Mutex<T>);

impl<T> LoggedMutex<T> {
	pub fn new(value: T) -> Self {
		Self(Mutex::new(value))
	}

	#[track_caller]
	pub fn lock(&self) -> impl Future<Output = LoggedMutexGuard<'_, T>> {
		let counter = LOCK_COUNTER.fetch_add(1, Ordering::Relaxed);

		log::trace!(
			"⏳ Acquiring mutex lock #{} for `{}` at {}...",
			counter,
			type_name::<T>(),
			std::panic::Location::caller()
		);

		async move {
			let lock = LoggedMutexGuard(self.0.lock().await, counter);

			log::trace!(
				"🔒 Acquired mutex lock #{} for `{}`",
				counter,
				type_name::<T>()
			);

			lock
		}
	}
}
