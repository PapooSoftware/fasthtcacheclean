// Copyright (c) 2022 Christoph Grenz (Papoo Software & Media GmbH) <info@papoo.de>
// SPDX-License-Identifier: LGPL-3.0-or-later

use std::fmt;

/// Statistic results
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
	pub deleted: u64,
	pub deleted_folders: u64,
	pub failed: u64,
}

impl Stats {
	/// Increment the failed counter
	#[inline]
	pub fn add_failed(&mut self) {
		self.failed += 1;
	}

	/// Count the given result into the statistics
	#[inline]
	pub fn count<E: fmt::Debug>(&mut self, r: Result<bool, E>) {
		match r {
			Ok(true) => self.deleted += 1,
			Ok(false) => {}
			Err(_) => {
				self.failed += 1;
			}
		}
	}

	/// Count the given result for folder deletion into the statistics
	#[inline]
	pub fn count_folder<E: fmt::Debug>(&mut self, r: Result<bool, E>) {
		match r {
			Ok(true) => self.deleted_folders += 1,
			Ok(false) => {}
			Err(_) => {
				self.failed += 1;
			}
		}
	}

	/// Merge the counts of the given stats-returning result into the statistics
	#[inline]
	pub fn merge_result<E: fmt::Debug>(&mut self, r: Result<Stats, E>) {
		match r {
			Ok(stats) => {
				self.deleted += stats.deleted;
				self.deleted_folders += stats.deleted_folders;
				self.failed += stats.failed;
			}
			Err(_) => self.failed += 1,
		}
	}

	/// Merge the counts of the given stats into the statistics
	#[inline]
	pub fn merge(&mut self, stats: Stats) {
		self.deleted += stats.deleted;
		self.deleted_folders += stats.deleted_folders;
		self.failed += stats.failed;
	}
}

impl std::iter::Sum<Self> for Stats {
	fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
		let mut acc = Self::default();
		for item in iter {
			acc.merge(item);
		}
		acc
	}
}

impl<E: fmt::Debug> std::iter::Sum<Result<Self, E>> for Stats {
	fn sum<I: Iterator<Item = Result<Self, E>>>(iter: I) -> Self {
		let mut acc = Self::default();
		for item in iter {
			acc.merge_result(item);
		}
		acc
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Tests `Stats` `Default` implementation
	#[test]
	fn test_stats_default() {
		let result = <Stats as Default>::default();
		assert_eq!(result.deleted, 0);
		assert_eq!(result.deleted_folders, 0);
		assert_eq!(result.failed, 0);
	}

	/// Tests `Stats` counting
	#[test]
	fn test_stats_counting() {
		let mut result = <Stats as Default>::default();
		result.count(Err(()));
		result.count(Err(()));
		result.count_folder(Err(()));
		result.count::<()>(Ok(true));
		result.count::<&'static str>(Ok(true));
		result.count::<i32>(Ok(false));
		result.count_folder::<()>(Ok(true));
		result.count::<bool>(Ok(false));
		result.add_failed();

		assert_eq!(result.deleted, 2);
		assert_eq!(result.deleted_folders, 1);
		assert_eq!(result.failed, 4);
	}

	/// Tests `Stats` summing
	#[test]
	fn test_stats_summing() {
		let items = [
			Stats {
				deleted: 50,
				deleted_folders: 3,
				failed: 12,
			},
			Stats {
				deleted: 20,
				deleted_folders: 2,
				failed: 29,
			},
			Stats {
				deleted: 0,
				deleted_folders: 0,
				failed: 0,
			},
			Stats {
				deleted: 0,
				deleted_folders: 0,
				failed: 1,
			},
		];

		let result: Stats = items.into_iter().sum();
		assert_eq!(result.deleted, 70);
		assert_eq!(result.deleted_folders, 5);
		assert_eq!(result.failed, 42);
	}

	/// Tests `Stats` summing
	#[test]
	fn test_stats_result_summing() {
		let items: [Result<Stats, ()>; 5] = [
			Ok(Stats {
				deleted: 50,
				deleted_folders: 3,
				failed: 12,
			}),
			Ok(Stats {
				deleted: 20,
				deleted_folders: 2,
				failed: 29,
			}),
			Ok(Stats {
				deleted: 0,
				deleted_folders: 0,
				failed: 0,
			}),
			Err(()),
			Ok(Stats {
				deleted: 0,
				deleted_folders: 0,
				failed: 1,
			}),
		];

		let result: Stats = items.into_iter().sum();
		assert_eq!(result.deleted, 70);
		assert_eq!(result.deleted_folders, 5);
		assert_eq!(result.failed, 43);
	}
}
