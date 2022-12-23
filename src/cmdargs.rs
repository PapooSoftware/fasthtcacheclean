// Copyright (c) 2022 Papoo Software & Media GmbH <info@papoo.de>
// SPDX-License-Identifier: LGPL-3.0-or-later

use crate::size_spec::SizeSpec;
use clap::Parser;
use std::path::PathBuf;

/// Program for cleaning the Apache disk cache.
#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
	/// Root directory of the disk cache.
	#[clap(short, long)]
	pub path: PathBuf,
	/// Minimum free disk space to keep. Attach 'K', 'M', 'G', 'T' or '%' to
	/// specify Kilobytes, Megabytes, Gigabytes, Terabytes or a percentage
	/// of the total disk size. Attach `Ki`, `Mi`, etc. to specify Kibibytes, Mebibytes, etc.
	#[clap(short='f', long, value_name="BYTES|PERCENT", default_value_t=SizeSpec::Percentage(10.0))]
	pub min_free_space: SizeSpec,

	/// Minimum free inodes to keep. Attach 'K', 'M', 'G', 'T' or '%' to
	/// specify thousands, millions, billions, trillions or a percentage
	/// of the total disk inodes.
	#[clap(short='F', long, value_name="COUNT|PERCENT", default_value_t=SizeSpec::Percentage(5.0))]
	pub min_free_inodes: SizeSpec,

	/// Jobs to run simultaneously. (0 for automatic selection based on available CPUs)
	///
	/// Use `-j1` for slow storage devices where parallel accesses slow down too much.
	#[clap(short, long, default_value_t = 0)]
	pub jobs: usize,

	/// Increase verbosity
	#[clap(short, long, action = clap::ArgAction::Count)]
	pub verbose: u8,
}
