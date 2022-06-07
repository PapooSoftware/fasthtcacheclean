use clap::Parser;
use nix::sys::statfs::statfs;
use rayon::prelude::*;
use std::cmp::max;
use std::collections::HashSet;
use std::fs::{remove_dir, remove_file, DirEntry, Metadata};
use std::num::ParseFloatError;
use std::num::ParseIntError;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;
use std::time::{Duration, SystemTime};
use std::{fmt, io};
use thiserror::Error;

mod apache_cache_header;

const CACHE_HEADER_SUFFIX: &str = ".header";
const CACHE_DATA_SUFFIX: &str = ".data";
const CACHE_VDIR_SUFFIX: &str = ".vary";
const CACHE_HEADER_VDIR_EXTENSION: &str = "header.vary";
const AP_TEMPFILE_BASE: &str = "aptmp";
const AP_TEMPFILE_SUFFIX: &str = "XXXXXX";

/// Command line arguments
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
	/// Root directory of the disk cache.
	#[clap(short, long)]
	path: PathBuf,
	/// Minimum free disk space to keep. Attach 'K', 'M', 'G', 'T' or '%' to
	/// specify Kilobytes, Megabytes, Gigabytes, Terabytes or a percentage
	/// of the total disk size.
	#[clap(short='f', long, value_name="BYTES|PERCENT", default_value_t=SizeSpec::Percentage(5.0))]
	min_free_space: SizeSpec,

	/// Minimum free disk space to keep. Attach 'K', 'M', 'G', 'T' or '%' to
	/// specify Kilobytes, Megabytes, Gigabytes, Terabytes or a percentage
	/// of the total disk inodes.
	#[clap(short='F', long, value_name="COUNT|PERCENT", default_value_t=SizeSpec::Percentage(1.0))]
	min_free_inodes: SizeSpec,
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
#[derive(Debug, Clone, Copy)]
struct Stats {
	deleted: u64,
	failed: u64,
}

impl Stats {
	/// Count the given result into the statistics
	#[inline]
	fn count<E: fmt::Debug>(&mut self, r: Result<bool, E>) {
		match r {
			Ok(true) => self.deleted += 1,
			Ok(false) => {}
			Err(_) => {
				self.failed += 1;
			}
		}
	}

	/// Merge the counts of the given stats-returning result into the statistics
	#[inline]
	fn merge_result<E: fmt::Debug>(&mut self, r: Result<Stats, E>) {
		match r {
			Ok(stats) => {
				self.deleted += stats.deleted;
				self.failed += stats.failed;
			}
			Err(_) => self.failed += 1,
		}
	}

	/// Merge the counts of the given stats into the statistics
	#[inline]
	fn merge(&mut self, stats: Stats) {
		self.deleted += stats.deleted;
		self.failed += stats.failed;
	}
}

impl Default for Stats {
	fn default() -> Self {
		Self {
			deleted: 0,
			failed: 0,
		}
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

/// Deletion condition
#[derive(Debug, Clone, Copy)]
enum Condition {
	Expired(Duration),
	Accessed(Duration),
	Modified(Duration),
}

impl Condition {
	/// Returns `Ok(true)` if the condition matches the file
	pub fn matches(
		&self,
		path: &Path,
		now: &SystemTime,
	) -> Result<bool, Box<dyn std::error::Error>> {
		match self {
			Condition::Expired(duration) => {
				let mut file = std::fs::File::open(&path)?;
				let expiry = apache_cache_header::read_expiration_time(&mut file)?;
				Ok(*now - *duration > expiry)
			}
			Condition::Accessed(duration) => {
				let metadata = path.metadata()?;
				Ok(*now - *duration > metadata.accessed()?)
			}
			Condition::Modified(duration) => {
				let metadata = path.metadata()?;
				Ok(*now - *duration > metadata.modified()?)
			}
		}
	}
}

impl fmt::Display for Condition {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Expired(d) => write!(f, "expired >= {:?}", d),
			Self::Accessed(d) => write!(f, "accessed >= {:?}", d),
			Self::Modified(d) => write!(f, "modified >= {:?}", d),
		}
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
) -> Result<bool, Box<dyn std::error::Error>> {
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
) -> Result<bool, Box<dyn std::error::Error>> {
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
	let result = remove_dir(entry.path());
	match result {
		Ok(()) => Ok(true),
		Err(e) if matches!(e.raw_os_error(), Some(libc::ENOTEMPTY)) => Ok(false),
		Err(e) => Err(From::from(e)),
	}
}

/// Deletes a data file, if there is a newer vary directory
fn delete_data_if_newer_vary(
	entry: &DirEntry,
	now: &SystemTime,
) -> Result<bool, Box<dyn std::error::Error>> {
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
fn process_header_file(
	entry: &DirEntry,
	now: &SystemTime,
	in_vary: bool,
	condition: Condition,
) -> Result<bool, Box<dyn std::error::Error>> {
	let path = entry.path();
	if let Ok(true) = condition.matches(&path, now) {
		if !in_vary {
			let mut vdir_path = path.clone();
			vdir_path.set_extension(CACHE_HEADER_VDIR_EXTENSION);
			if vdir_path.exists() {
				if let Ok(metadata) = vdir_path.metadata() {
					if metadata.is_dir() {
						// Don't delete main header as long as a vary directory exists
						return Ok(false);
					}
				}
			}
		}
		let mut data_path = path.clone();
		data_path.set_extension(&CACHE_DATA_SUFFIX[1..]);
		let _ = remove_file(data_path);
		remove_file(path).map(|_| true).map_err(From::from)
	} else {
		Ok(false)
	}
}

/// Processes the subfolders of a folder in parallel
fn process_folder_parallel(
	path: &Path,
	now: &SystemTime,
	condition: Condition,
) -> Result<Stats, Box<dyn std::error::Error>> {
	let mut stats = Stats::default();

	// First clean old temporary files
	for item in path.read_dir()? {
		if let Ok(item) = item {
			if let Some(name) = item.file_name().to_str() {
				// Temporary files -> only delete if old
				if name.len() == AP_TEMPFILE_BASE.len() + AP_TEMPFILE_SUFFIX.len()
					&& name.starts_with(AP_TEMPFILE_BASE)
				{
					stats.count(delete_file_if_not_recent(&item, now, 600));
				}
			}
		}
	}

	// Run `process_folder` in parallel (in up to CPUs/2 threads)
	stats.merge(
		path.read_dir()?
			.collect::<Vec<_>>()
			.into_par_iter()
			.map(|item| process_folder(&item?.path(), now, false, condition))
			.sum(),
	);
	Ok(stats)
}

/// Processes one folder recursively
fn process_folder(
	path: &Path,
	now: &SystemTime,
	in_vary: bool,
	condition: Condition,
) -> Result<Stats, io::Error> {
	let mut known_headers = HashSet::new();
	let mut stats = Stats::default();

	for item in path.read_dir()? {
		if let Ok(item) = item {
			let name = item.file_name();
			if let Some(name) = name.to_str() {
				// Temporary files -> only delete if old
				if name.len() == AP_TEMPFILE_BASE.len() + AP_TEMPFILE_SUFFIX.len()
					&& name.starts_with(AP_TEMPFILE_BASE)
				{
					stats.count(delete_file_if_not_recent(&item, now, 600));
				}
				// Header files
				else if name.ends_with(CACHE_HEADER_SUFFIX) {
					known_headers.insert(name[..name.len() - CACHE_HEADER_SUFFIX.len()].to_owned());
					stats.count(process_header_file(&item, now, in_vary, condition));
				}
				// Data files
				else if name.ends_with(CACHE_DATA_SUFFIX) {
					let base = &name[..name.len() - CACHE_DATA_SUFFIX.len()];
					if !known_headers.contains(base) {
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
					let _ = process_folder(&item.path(), now, true, condition);
					stats.count(delete_folder_if_not_recent(&item, None, now, 1800));
				}
				// Recurse into other directories
				else if let Ok(metadata) = item.metadata() {
					if metadata.is_dir() {
						stats.merge_result(process_folder(&item.path(), now, in_vary, condition));
						stats.count(delete_folder_if_not_recent(
							&item,
							Some(metadata),
							now,
							1800,
						));
					}
				}
			}
		}
	}

	// Be somewhat nice to other processes by yielding the CPU after each non-vary directory
	if !in_vary {
		thread::yield_now();
	}

	Ok(stats)
}

/// Calculates a percentage of how close the used space is to the free space/inode limit
///
/// Returns the maximum of space and inode percentage.
fn calculate_usage(minspace: SizeSpec, mininodes: SizeSpec) -> f64 {
	let fsstat = statfs(".").expect("Couldn't get free space information");
	let block_size: u64 = fsstat.block_size().try_into().unwrap_or(4096);
	let free_space_target = minspace.value(block_size * fsstat.blocks());
	let free_space = fsstat.blocks_free() * block_size;
	let free_inodes_target = mininodes.value(fsstat.files());
	let free_inodes = fsstat.files_free();

	let inode_usage = free_inodes_target as f64 * 100.0 / (free_inodes + 1) as f64;
	let space_usage = free_space_target as f64 * 100.0 / (free_space + 1) as f64;
	if inode_usage > space_usage {
		inode_usage
	} else {
		space_usage
	}
}

/// Main function
///
/// Parses the arguments and runs the cleanup
fn main() {
	let args = Args::parse();

	let cpus = num_cpus::get();
	let threads = max(1, cpus / 2);
	rayon::ThreadPoolBuilder::new()
		.num_threads(threads)
		.build_global()
		.unwrap();

	std::env::set_current_dir(args.path).expect("Couldn't change to cache directory.");
	let now = SystemTime::now();

	let usage = calculate_usage(args.min_free_space, args.min_free_inodes);

	println!("Usage: {:.1}% of target space/inode limit", usage);

	if usage >= 99.5 {
		// Run through these conditions until there is enough free space
		for condition in [
			Condition::Expired(Duration::new(3600, 0)),
			Condition::Expired(Duration::new(1800, 0)),
			Condition::Expired(Duration::new(600, 0)),
			Condition::Expired(Duration::new(60, 0)),
			Condition::Accessed(Duration::new(1800, 0)),
			Condition::Accessed(Duration::new(600, 0)),
			Condition::Accessed(Duration::new(120, 0)),
			Condition::Modified(Duration::new(600, 0)),
			Condition::Modified(Duration::new(120, 0)),
		] {
			println!("Deleting cache contents with {}...", condition);
			let result = process_folder_parallel(".".as_ref(), &now, condition);

			if let Ok(stats) = result {
				println!("{:?}", stats);
			}

			if calculate_usage(args.min_free_space, args.min_free_inodes) < 100.0 {
				break;
			}
		}
	} else if usage > 95.0 {
		let condition = Condition::Expired(Duration::new(10800, 0));
		println!("Deleting cache contents with {}...", condition);
		let result = process_folder_parallel(".".as_ref(), &now, condition);
		if let Ok(stats) = result {
			println!("{:?}", stats);
		}
	} else if usage > 90.0 {
		let condition = Condition::Expired(Duration::new(21600, 0));
		println!("Deleting cache contents with {}...", condition);
		let result = process_folder_parallel(".".as_ref(), &now, condition);
		if let Ok(stats) = result {
			println!("{:?}", stats);
		}
	} else {
		let condition = Condition::Expired(Duration::new(86400, 0));
		println!("Deleting cache contents with {}...", condition);
		let result = process_folder_parallel(".".as_ref(), &now, condition);
		if let Ok(stats) = result {
			println!("{:?}", stats);
		}
	}
}
