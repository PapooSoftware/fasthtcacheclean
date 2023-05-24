// Copyright (c) 2022 Papoo Software & Media GmbH <info@papoo.de>
// SPDX-License-Identifier: LGPL-3.0-or-later

#[macro_use]
extern crate tracing;

use crossbeam::{channel, thread};
use nix::sys::statfs::statfs;
use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use std::collections::HashSet;
use std::convert::Infallible;
use std::error::Error;
use std::fs::{remove_dir, remove_file, DirEntry, Metadata};
use std::io;
use std::mem::drop;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Mutex;
use std::thread::yield_now;
use std::time::{Instant, SystemTime};

mod apache_cache;
mod cache_file_info;
mod cache_priority_queue;
mod config;
mod size_spec;
mod stats;

pub use cache_file_info::CacheFileInfo;
pub use cache_priority_queue::CachePriorityQueue;
pub use config::Config;
pub use size_spec::SizeSpec;
pub use stats::Stats;

pub const MAX_DELETE_COUNT: usize = 1000000;

const CACHE_HEADER_SUFFIX: &str = ".header";
const CACHE_DATA_SUFFIX: &str = ".data";
const CACHE_VDIR_SUFFIX: &str = ".vary";
const CACHE_HEADER_VDIR_EXTENSION: &str = "header.vary";
const AP_TEMPFILE_BASE: &str = "aptmp";
const AP_TEMPFILE_SUFFIX: &str = "XXXXXX";

/// Deletes a file, if it wasn't modified or accessed recently
///
/// Should only be called for "aptmp" and orphaned data files.
pub fn delete_file_if_not_recent(
	entry: &DirEntry,
	now: &SystemTime,
	seconds: u64,
) -> Result<bool, io::Error> {
	let metadata = entry.metadata()?;
	if !metadata.is_file() {
		return Ok(false);
	}
	match now.duration_since(metadata.modified()?) {
		Ok(duration) if duration.as_secs() >= seconds => {}
		_ => {
			return Ok(false);
		}
	}
	match now.duration_since(metadata.accessed()?) {
		Ok(duration) if duration.as_secs() >= seconds => {}
		_ => {
			return Ok(false);
		}
	}
	let path = entry.path();
	let result = remove_file(&path);

	debug!(
		path=?&path,
		error=result.as_ref().err().map(|v| v as &dyn Error),
		"Deleting file {:?}: {}", path, if result.is_ok() {"ok"} else {"failed"}
	);

	result.map(|_| true)
}

/// Deletes an empty folder, if it wasn't modified or accessed recently
fn delete_folder_if_not_recent(
	entry: &DirEntry,
	metadata: Option<Metadata>,
	now: &SystemTime,
	seconds: u64,
) -> Result<bool, io::Error> {
	let metadata = match metadata {
		Some(m) => m,
		None => entry.metadata()?,
	};

	// Abort if it isn't a directory
	if !metadata.is_dir() {
		return Ok(false);
	}

	// Abort if it has subfolders (optimization)
	if metadata.nlink() > 2 {
		return Ok(false);
	}

	// Check if it was modified in the last `seconds`
	match now.duration_since(metadata.modified()?) {
		Ok(duration) if duration.as_secs() >= seconds => {}
		_ => {
			return Ok(false);
		}
	}

	// Check if it was accessed in the last `seconds`
	match now.duration_since(metadata.accessed()?) {
		Ok(duration) if duration.as_secs() >= seconds => {}
		_ => {
			return Ok(false);
		}
	}

	// Try to remove it
	let path = entry.path();
	let result = remove_dir(&path);
	if result.is_ok() {
		debug!(
			path=?&path,
			"Deleting folder {:?}: ok", path
		);
	}
	match result {
		Ok(()) => Ok(true),
		Err(e) if matches!(e.raw_os_error(), Some(libc::ENOTEMPTY)) => Ok(false),
		Err(e) => Err(e),
	}
}

/// Processes a header file
///
/// Returns `true` if the file and its associated data file were successfully deleted.
#[inline]
pub fn process_header_file(fileinfo: &CacheFileInfo) -> Result<bool, io::Error> {
	let data_path = fileinfo.data_path();
	if remove_file(&data_path).is_ok() {
		debug!(
			path=?data_path,
			"Deleting data file {:?}: ok", data_path,
		);
	}

	let path = fileinfo.header_path();
	let result = remove_file(path);

	debug!(
		path=?path,
		error=result.as_ref().err().map(|v| v as &dyn Error),
		"Deleting header file {:?}: {}", path, if result.is_ok() {"ok"} else {"failed"}
	);

	result.map(|_| true)
}

/// Processes the subfolders of a folder in parallel
///
/// Directly deletes definitely unneccessary files and folders, then collects
/// information about all valid cache entries, prunes them and returns statistics.
pub fn process_folder_parallel(path: &Path, config: &Config, now: &SystemTime) -> Result<Stats, io::Error> {
	let mut stats = Stats::default();

	debug!("Cleaning up temporary files...");
	let start = Instant::now();
	// First clean old temporary files
	for item in path.read_dir()?.flatten() {
		if let Some(name) = item.file_name().to_str() {
			// Temporary files -> only delete if old
			if name.len() == AP_TEMPFILE_BASE.len() + AP_TEMPFILE_SUFFIX.len()
				&& name.starts_with(AP_TEMPFILE_BASE)
			{
				stats.count(delete_file_if_not_recent(&item, now, 600));
			}
		}
	}
	debug!("Cleanup done ({:.2}s).", start.elapsed().as_secs_f64());

	let mut folders = path.read_dir()?.collect::<Vec<_>>();
	let chunk_size = (folders.len() / config.jobs) + 1;
	let stats = Mutex::new(stats);
	let mut queue = CachePriorityQueue::with_capacity(1000, MAX_DELETE_COUNT);

	// Shuffle the subfolders to evenly distribute to the threads
	let mut rng = thread_rng();
	folders.shuffle(&mut rng);

	debug!("Scanning directories... ({} threads)", config.jobs);
	let start = Instant::now();
	// Run `process_folder` in parallel (in up to CPUs/2 threads)
	thread::scope(|s| {
		let (sender, receiver) = channel::bounded(1000);

		for chunk in folders.chunks(chunk_size) {
			let sender = sender.clone();
			let stats = &stats;
			s.spawn(move |_| {
				for folder in chunk.iter().flatten() {
					let result = process_folder(&folder.path(), config, now, &sender);
					stats.lock().unwrap().merge_result(result);
				}
			});
		}
		drop(sender);

		for fileinfo in receiver {
			queue.push(fileinfo);
		}
	})
	.unwrap();
	debug!("Scanning done ({:.2}s).", start.elapsed().as_secs_f64());
	let mut stats = stats.into_inner().unwrap();

	debug!("Deleting cache entries...");
	let start = Instant::now();
	let results = queue.into_sorted_vec();
	for chunk in results.chunks(10) {
		for fileinfo in chunk {
			stats.count(process_header_file(fileinfo));
		}
		let usage = calculate_usage(config.min_free_space, config.min_free_inodes);
		if usage < 99.0 || (usage < 99.5 && rng.gen::<u8>() < 1) {
			break;
		}
		yield_now();
	}
	debug!("Deleting done ({:.2}s).", start.elapsed().as_secs_f64());

	Ok(stats)
}

/// Processes one folder recursively
///
/// Directly deletes definitely unneccessary files and folders and
/// sends information about all valid cache entries via `sender`.
///
/// Activates `desperate` mode if usage is over 105 % of the limits
/// in `config`.
pub fn process_folder(
	path: &Path,
	config: &Config,
	now: &SystemTime,
	sender: &channel::Sender<CacheFileInfo>,
) -> Result<Stats, io::Error> {
	let mut stats = Stats::default();
	let usage = calculate_usage(config.min_free_space, config.min_free_inodes);
	let desperate = usage > 105.0;

	stats.merge(scan_folder(path, now, false, sender, desperate)?);

	Ok(stats)
}

/// Scans one subfolder recursively
///
/// Directly deletes definitely unneccessary files and folders and
/// sends information about all valid cache entries via `sender`.
///
/// If `desperate` is true, deleting happens more aggressively.
#[instrument(level = "trace", skip(now, sender))]
pub fn scan_folder(
	path: &Path,
	now: &SystemTime,
	in_vary: bool,
	sender: &channel::Sender<CacheFileInfo>,
	desperate: bool,
) -> Result<Stats, io::Error> {
	let mut known_headers = HashSet::new();
	let mut stats = Stats::default();

	for item in path.read_dir()?.flatten() {
		let name = item.file_name();
		if let Some(name) = name.to_str() {
			// Temporary files -> only delete if old
			if name.len() == AP_TEMPFILE_BASE.len() + AP_TEMPFILE_SUFFIX.len()
				&& name.starts_with(AP_TEMPFILE_BASE)
			{
				stats.count(delete_file_if_not_recent(&item, now, 600));
			}
			// Header files
			else if let Some(stem) = name.strip_suffix(CACHE_HEADER_SUFFIX) {
				known_headers.insert(stem.to_owned());
				if let Ok(fileinfo) = CacheFileInfo::new(&item) {
					if !in_vary && fileinfo.is_vary() {
						// Delete orphaned data file if the header indicates a vary directory
						let data_path = fileinfo.data_path();
						let result = remove_file(&data_path);
						stats.count::<Infallible>(Ok(result.is_ok()));

						if result.is_ok() {
							debug!(
								path=?&data_path,
								"Deleting orphaned data file {:?}: ok", &data_path,
							);
						}

						// Don't delete main header as long as a vary directory exists (as long as not in desperate mode)
						if !desperate {
							let vdir_path = fileinfo.vary_path();
							if vdir_path.exists() {
								if let Ok(metadata) = vdir_path.metadata() {
									if metadata.is_dir() && metadata.nlink() > 2 {
										continue;
									}
								}
							}
						}
					}
					sender.send(fileinfo).unwrap();
				} else {
					stats.add_failed();
				}
			}
			// Data files
			else if let Some(stem) = name.strip_suffix(CACHE_DATA_SUFFIX) {
				if !known_headers.contains(stem) {
					let mut header_path = item.path();
					header_path.set_extension(&CACHE_HEADER_SUFFIX[1..]);
					// If the header file is missing and the file is old, delete it.
					if !header_path.exists() {
						stats.count(delete_file_if_not_recent(&item, now, 120));
						continue;
					}
				}
			}
			// Recurse into vary directories
			else if name.ends_with(CACHE_VDIR_SUFFIX) {
				stats.merge_result(scan_folder(&item.path(), now, true, sender, desperate));
				stats.count_folder(delete_folder_if_not_recent(&item, None, now, 300));
			}
			// Recurse into other directories
			else if let Ok(metadata) = item.metadata() {
				if metadata.is_dir() {
					stats.merge_result(scan_folder(&item.path(), now, in_vary, sender, desperate));
					stats.count_folder(delete_folder_if_not_recent(
						&item,
						Some(metadata),
						now,
						300,
					));
				}
			}
		}
	}

	// Be somewhat nice to other processes by yielding the CPU after each non-vary directory
	if !in_vary {
		yield_now();
	}

	Ok(stats)
}

/// Calculates a percentage of how close the used space is to the free space/inode limit
///
/// Returns the maximum of space and inode percentage.
pub fn calculate_usage(minspace: SizeSpec, mininodes: SizeSpec) -> f64 {
	let fsstat = statfs(".").expect("Couldn't get free space information");
	let block_size: u64 = fsstat.block_size().try_into().unwrap_or(4096);
	let total_space = block_size * fsstat.blocks();
	let used_space_target = total_space.saturating_sub(minspace.value(total_space));
	let used_space = fsstat.blocks().saturating_sub(fsstat.blocks_available()) * block_size;

	let total_inodes = fsstat.files();
	let used_inodes_target = total_inodes.saturating_sub(mininodes.value(total_inodes));
	let used_inodes = total_inodes.saturating_sub(fsstat.files_free());

	let inode_usage = used_inodes as f64 * 100.0 / (used_inodes_target + 1) as f64;
	let space_usage = used_space as f64 * 100.0 / (used_space_target + 1) as f64;
	if inode_usage > space_usage {
		inode_usage
	} else {
		space_usage
	}
}
