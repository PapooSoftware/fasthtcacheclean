#[macro_use]
extern crate log;

use clap::Parser;
use crossbeam::thread;
use nix::sys::statfs::statfs;
use rand::seq::SliceRandom;
use rand::thread_rng;
use std::cmp::max;
use std::collections::HashSet;
use std::fs::{remove_dir, remove_file, DirEntry, Metadata};
use std::mem::drop;
use std::num::ParseFloatError;
use std::num::ParseIntError;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Mutex;
use std::thread::yield_now;
use std::time::{Instant, SystemTime};
use std::{fmt, io};
use thiserror::Error;

mod apache_cache_header;
mod cache_file_info;
mod cache_priority_queue;

use cache_file_info::CacheFileInfo;
use cache_priority_queue::CachePriorityQueue;

#[cfg(test)]
mod tests;

const MAX_DELETE_COUNT: usize = 1000000;

const CACHE_HEADER_SUFFIX: &str = ".header";
const CACHE_DATA_SUFFIX: &str = ".data";
const CACHE_VDIR_SUFFIX: &str = ".vary";
const CACHE_HEADER_VDIR_EXTENSION: &str = "header.vary";
const AP_TEMPFILE_BASE: &str = "aptmp";
const AP_TEMPFILE_SUFFIX: &str = "XXXXXX";

/// Program for cleaning the Apache disk cache.
#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
struct Args {
	/// Root directory of the disk cache.
	#[clap(short, long, parse(from_os_str))]
	path: PathBuf,
	/// Minimum free disk space to keep. Attach 'K', 'M', 'G', 'T' or '%' to
	/// specify Kilobytes, Megabytes, Gigabytes, Terabytes or a percentage
	/// of the total disk size.
	#[clap(short='f', long, value_name="BYTES|PERCENT", default_value_t=SizeSpec::Percentage(10.0))]
	min_free_space: SizeSpec,

	/// Minimum free disk space to keep. Attach 'K', 'M', 'G', 'T' or '%' to
	/// specify Kilobytes, Megabytes, Gigabytes, Terabytes or a percentage
	/// of the total disk inodes.
	#[clap(short='F', long, value_name="COUNT|PERCENT", default_value_t=SizeSpec::Percentage(5.0))]
	min_free_inodes: SizeSpec,

	/// Jobs to run simultaneously. (0 for automatic selection based on available CPUs)
	///
	/// Use `-j1` for slow storage devices where parallel accesses slow down too much.
	#[clap(short, long, default_value_t = 0)]
	jobs: usize,

	/// Increase verbosity
	#[clap(short, long, action = clap::ArgAction::Count)]
	verbose: u8,
}

/// Representation for a user-specified size limit (percentage or absolute)
#[derive(Debug, Clone, Copy)]
enum SizeSpec {
	Percentage(f64),
	Absolute(u64),
}

impl fmt::Display for SizeSpec {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			SizeSpec::Percentage(n) => write!(f, "{}%", n),
			SizeSpec::Absolute(n) => {
				let n = *n;
				if n < 1000 {
					write!(f, "{}", n)
				} else if n < 1000000 {
					write!(f, "{}K", (n as f64) / 1000.0)
				} else if n < 1000000000 {
					write!(f, "{}M", (n as f64) / 1000000.0)
				} else if n < 1000000000000 {
					write!(f, "{}G", (n as f64) / 1000000000.0)
				} else {
					write!(f, "{}T", (n as f64) / 1000000000000.0)
				}
			}
		}
	}
}

impl SizeSpec {
	/// Return the absolute value when given the available `total`
	#[inline]
	pub fn value(&self, total: u64) -> u64 {
		match self {
			SizeSpec::Percentage(n) => (*n / 100.0 * (total as f64)) as u64,
			SizeSpec::Absolute(n) => *n,
		}
	}
}

/// Statistic results
#[derive(Debug, Clone, Copy, Default)]
struct Stats {
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

/// Error type for parsing a `SizeSpec`
#[derive(Error, Debug)]
pub enum ParseSizeSpecError {
	#[error("expected a positive numeric value with an optional unit")]
	EmptyString,
	#[error("expected a positive numeric value with an optional unit")]
	InvalidInt(#[from] ParseIntError),
	#[error("expected a positive numeric value with an optional unit")]
	InvalidFloat(#[from] ParseFloatError),
	#[error("`{0}` is not a valid unit. Known units are `K`, `M`, `G`, `T`, `%`.")]
	InvalidUnit(char),
}

/// Parsing a string into a `SizeSpec`
impl FromStr for SizeSpec {
	type Err = ParseSizeSpecError;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let first_char = s.chars().next().ok_or(ParseSizeSpecError::EmptyString)?;
		let last_char = s
			.chars()
			.rev()
			.next()
			.ok_or(ParseSizeSpecError::EmptyString)?;

		if first_char == '-' {
			return Err(s.parse::<u64>().unwrap_err().into());
		}

		match last_char {
			'0'..='9' => Ok(SizeSpec::Absolute(s.parse::<u64>()?)),
			'K' | 'k' => Ok(SizeSpec::Absolute(
				(s[..s.len() - 1].parse::<f64>()? * 1000.0) as u64,
			)),
			'M' => Ok(SizeSpec::Absolute(
				(s[..s.len() - 1].parse::<f64>()? * 1000000.0) as u64,
			)),
			'G' => Ok(SizeSpec::Absolute(
				(s[..s.len() - 1].parse::<f64>()? * 1000000000.0) as u64,
			)),
			'T' => Ok(SizeSpec::Absolute(
				(s[..s.len() - 1].parse::<f64>()? * 1000000000000.0) as u64,
			)),
			'%' => Ok(SizeSpec::Percentage(s[..s.len() - 1].parse::<f64>()?)),
			_ => Err(ParseSizeSpecError::InvalidUnit(last_char)),
		}
	}
}

/// Deletes a file, if it wasn't modified or accessed recently
///
/// Should only be called for "aptmp" and orphaned data files.
fn delete_file_if_not_recent(
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
	remove_file(entry.path()).map(|_| true).map_err(From::from)
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
	match result {
		Ok(()) => Ok(true),
		Err(e) if matches!(e.raw_os_error(), Some(libc::ENOTEMPTY)) => Ok(false),
		Err(e) => Err(e),
	}
}

/// Deletes a data file, if there is a newer vary directory
fn delete_data_if_newer_vary(entry: &DirEntry, now: &SystemTime) -> Result<bool, io::Error> {
	let mut vary_header_path = entry.path();
	vary_header_path.set_extension(CACHE_HEADER_VDIR_EXTENSION);
	if vary_header_path.exists() {
		let vary_modified = vary_header_path.metadata()?.modified()?;
		let data_modified = entry.metadata()?.modified()?;
		if vary_modified > data_modified {
			return delete_file_if_not_recent(entry, now, 60);
		}
	}
	Ok(false)
}

/// Processes a header file
fn process_header_file(fileinfo: &CacheFileInfo) -> Result<bool, io::Error> {
	let data_path = fileinfo.data_path();
	let _ = remove_file(data_path);
	remove_file(fileinfo.header_path())
		.map(|_| true)
		.map_err(From::from)
}

/// Processes the subfolders of a folder in parallel
fn process_folder_parallel(path: &Path, args: &Args, now: &SystemTime) -> Result<Stats, io::Error> {
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
	let chunk_size = (folders.len() / args.jobs) + 1;
	let stats = Mutex::new(stats);
	let mut queue = CachePriorityQueue::with_capacity(1000, MAX_DELETE_COUNT);

	// Shuffle the subfolders to evenly distribute to the threads
	let mut rng = thread_rng();
	folders.shuffle(&mut rng);

	debug!("Scanning directories... ({} threads)", args.jobs);
	let start = Instant::now();
	// Run `process_folder` in parallel (in up to CPUs/2 threads)
	thread::scope(|s| {
		let (sender, receiver) = sync_channel(1000);

		for chunk in folders.chunks(chunk_size) {
			let sender = sender.clone();
			let stats = &stats;
			s.spawn(move |_| {
				for folder in chunk.iter().flatten() {
					let result = process_folder(&folder.path(), args, now, &sender);
					stats.lock().unwrap().merge_result(result);
				}
			});
		}
		drop(sender);

		while let Ok(fileinfo) = receiver.recv() {
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
		let usage = calculate_usage(args.min_free_space, args.min_free_inodes);
		if usage < 99.0 {
			break;
		}
		yield_now();
	}
	debug!("Deleting done ({:.2}s).", start.elapsed().as_secs_f64());

	Ok(stats)
}

/// Processes one folder recursively
fn process_folder(
	path: &Path,
	args: &Args,
	now: &SystemTime,
	sender: &SyncSender<CacheFileInfo>,
) -> Result<Stats, io::Error> {
	let mut stats = Stats::default();

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

	let usage = calculate_usage(args.min_free_space, args.min_free_inodes);
	let desperate = usage > 105.0;

	stats.merge(scan_folder(path, now, false, sender, desperate)?);

	Ok(stats)
}

/// Scans one subfolder recursively
///
/// Directly deletes definitely unneccessary files and folders and
/// inserts all valid cache entries into `queue`.
fn scan_folder(
	path: &Path,
	now: &SystemTime,
	in_vary: bool,
	sender: &SyncSender<CacheFileInfo>,
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
					if !in_vary && !desperate {
						let vdir_path = fileinfo.vary_path();
						if vdir_path.exists() {
							if let Ok(metadata) = vdir_path.metadata() {
								if metadata.is_dir() && metadata.nlink() > 2 {
									// Don't delete main header as long as a vary directory exists
									continue;
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
				if !in_vary {
					// Delete old data file if there is a newer Vary directory
					stats.count(delete_data_if_newer_vary(&item, now));
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
fn calculate_usage(minspace: SizeSpec, mininodes: SizeSpec) -> f64 {
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

fn init_logging(args: &Args) {
	#![cfg(feature = "systemd")]
	if systemd_journal_logger::connected_to_journal() {
		systemd_journal_logger::init().unwrap();

		log::set_max_level(match args.verbose {
			0 => log::LevelFilter::Warn,
			1 => log::LevelFilter::Info,
			2 => log::LevelFilter::Debug,
			_ => log::LevelFilter::Trace,
		});
		return;
	}

	let mut logger = env_logger::Builder::from_default_env();
	if args.verbose > 0 {
		logger.filter_level(match args.verbose {
			1 => log::LevelFilter::Info,
			2 => log::LevelFilter::Debug,
			_ => log::LevelFilter::Trace,
		});
	}
	logger.init();
}

/// Main function
///
/// Parses the arguments, initializes logging and runs the cleanup job
fn main() {
	// Parse command line arguments
	let mut args = Args::parse();

	// Initialize logging
	init_logging(&args);

	// Calculate number of threads if set to "auto"
	if args.jobs == 0 {
		let cpus = num_cpus::get();
		args.jobs = max(1, cpus / 2)
	}

	std::env::set_current_dir(&args.path).expect("Couldn't change to cache directory.");
	let now = SystemTime::now();

	let usage = calculate_usage(args.min_free_space, args.min_free_inodes);
	info!("Usage: {:.1}% of target space/inode limit", usage);

	if usage >= 90.0 {
		info!("Pruning cache...");

		let result = process_folder_parallel(".".as_ref(), &args, &now);

		if let Ok(stats) = result {
			info!(
				"Statistics: {} deleted files, {} deleted folders, {} failed to delete",
				stats.deleted, stats.deleted_folders, stats.failed
			);
		}
	} else {
		// do nothing
	}
}
