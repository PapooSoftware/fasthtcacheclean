// Copyright (c) 2022 Christoph Grenz (Papoo Software & Media GmbH) <info@papoo.de>
// SPDX-License-Identifier: LGPL-3.0-or-later

#[macro_use]
extern crate tracing;

mod cmdargs;
mod job_count;

use fasthtcacheclean::{Config, SizeSpec, calculate_usage, process_folder_parallel};
use clap::Parser;
use std::cmp::max;
use std::env;
use std::time::SystemTime;
use cmdargs::Args;

/// Initialize logging/tracing
fn init_logging(args: &Args) {
	use tracing_subscriber::filter::LevelFilter;
	use tracing_subscriber::fmt::format::FmtSpan;
	use tracing_subscriber::prelude::*;
	use tracing_subscriber::{fmt, EnvFilter};

	let filter_layer = EnvFilter::builder()
		.with_default_directive(
			match args.verbose {
				0 => LevelFilter::WARN,
				1 => LevelFilter::INFO,
				2 => LevelFilter::DEBUG,
				_ => LevelFilter::TRACE,
			}
			.into(),
		)
		.from_env_lossy();

	#[cfg(feature = "systemd")]
	if env::var_os("JOURNAL_STREAM").is_some() {
		if let Ok(journal_layer) = tracing_journald::layer() {
			tracing_subscriber::registry()
				.with(filter_layer)
				.with(journal_layer)
				.init();
			return;
		}
	}

	let fmt_layer = fmt::layer()
		.with_target(false)
		.with_span_events(FmtSpan::NONE);

	tracing_subscriber::registry()
		.with(filter_layer)
		.with(fmt_layer)
		.init();
}

impl Args {
	pub fn into_config<F: FnOnce() -> usize>(self, job_count_closure: F) -> Config {
		Config {
			path: self.path,
			min_free_space: self.min_free_space,
			min_free_inodes: self.min_free_inodes,
			jobs: self.jobs.unwrap_or_else(job_count_closure)
		}
	}
}

/// Main function
///
/// Parses the arguments, initializes logging and runs the cleanup job
fn main() {
	// Parse command line arguments
	let args = Args::parse();

	// Initialize logging
	init_logging(&args);

	// Create application configuration, calculating number of threads if set to "auto"
	let config = args.into_config(|| max(1, num_cpus::get() / 2));

	std::env::set_current_dir(&config.path).expect("Couldn't change to cache directory.");
	let now = SystemTime::now();

	let usage = calculate_usage(config.min_free_space, config.min_free_inodes);
	info!("Usage: {:.1}% of target space/inode limit", usage);

	if usage >= 90.0 {
		info!("Pruning cache...");

		let result = process_folder_parallel(".".as_ref(), &config, &now);

		if let Ok(stats) = result {
			let usage = calculate_usage(config.min_free_space, config.min_free_inodes);
			info!("Usage: {:.1}% of target space/inode limit", usage);
			info!(
				"Statistics: {} deleted files, {} deleted folders, {} failed to delete",
				stats.deleted, stats.deleted_folders, stats.failed
			);
		}
	} else {
		// do nothing
	}
}
