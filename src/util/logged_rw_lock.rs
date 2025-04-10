use std::{
	any::type_name,
	ops::{Deref, DerefMut},
	sync::atomic::Ordering,
};

use tokio::sync::RwLock;

use crate::util::LOCK_COUNTER;

#[allow(dead_code)]
pub struct LoggedRwLockReadGuard<'a, T: ?Sized>(
	tokio::sync::RwLockReadGuard<'a, T>,
	usize,
);

impl<T: ?Sized> Drop for LoggedRwLockReadGuard<'_, T> {
	fn drop(&mut self) {
		log::trace!(
			"🔓 Dropped read lock #{} for `{}`",
			self.1,
			type_name::<T>()
		);
	}
}

impl<T: ?Sized> Deref for LoggedRwLockReadGuard<'_, T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		self.0.deref()
	}
}

#[allow(dead_code)]
pub struct LoggedRwLockWriteGuard<'a, T: ?Sized>(
	tokio::sync::RwLockWriteGuard<'a, T>,
	usize,
);

impl<T: ?Sized> Drop for LoggedRwLockWriteGuard<'_, T> {
	fn drop(&mut self) {
		log::trace!(
			"🔓 Dropped read lock #{} for `{}`",
			self.1,
			type_name::<T>()
		);
	}
}

impl<T: ?Sized> Deref for LoggedRwLockWriteGuard<'_, T> {
	type Target = T;
	fn deref(&self) -> &Self::Target {
		self.0.deref()
	}
}

impl<T: ?Sized> DerefMut for LoggedRwLockWriteGuard<'_, T> {
	fn deref_mut(&mut self) -> &mut Self::Target {
		self.0.deref_mut()
	}
}

#[derive(Debug)]
pub struct LoggedRwLock<T>(RwLock<T>);

impl<T> LoggedRwLock<T> {
	pub fn new(value: T) -> Self {
		Self(RwLock::new(value))
	}

	#[track_caller]
	pub fn read(&self) -> impl Future<Output = LoggedRwLockReadGuard<'_, T>> {
		let counter = LOCK_COUNTER.fetch_add(1, Ordering::Relaxed);

		log::trace!(
			"⏳ Acquiring read lock #{} for `{}` at {}...",
			counter,
			type_name::<T>(),
			std::panic::Location::caller()
		);

		async move {
			let lock = LoggedRwLockReadGuard(self.0.read().await, counter);

			log::trace!(
				"🔒 Acquired read lock #{} for `{}`",
				counter,
				type_name::<T>()
			);

			lock
		}
	}

	#[track_caller]
	pub fn write(&self) -> impl Future<Output = LoggedRwLockWriteGuard<'_, T>> {
		let counter = LOCK_COUNTER.fetch_add(1, Ordering::Relaxed);

		log::trace!(
			"⏳ Acquiring write lock #{} for `{}` at {}...",
			counter,
			type_name::<T>(),
			std::panic::Location::caller()
		);

		async move {
			let lock = LoggedRwLockWriteGuard(self.0.write().await, counter);

			log::trace!(
				"🔒 Acquired write lock #{} for `{}`",
				counter,
				type_name::<T>()
			);

			lock
		}
	}
}
