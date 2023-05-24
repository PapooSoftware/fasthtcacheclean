// Copyright (c) 2022 Papoo Software & Media GmbH <info@papoo.de>
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::size_spec::SizeSpec;
use std::path::PathBuf;

/// Application configuration parameters
#[derive(Debug, Clone)]
pub struct Config {
	/// Root directory of the disk cache
	pub path: PathBuf,

	/// Minimum free disk space to keep
	pub min_free_space: SizeSpec,

	/// Minimum free inodes to keep
	pub min_free_inodes: SizeSpec,

	/// Jobs to run simultaneously
	pub jobs: usize,
}

