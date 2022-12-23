use std::cmp::{max, Eq, Ord, Ordering, PartialEq, PartialOrd};
use std::fs::{DirEntry, OpenOptions};
use std::io::Error as IOError;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::apache_cache;
use crate::CACHE_DATA_SUFFIX;
use crate::CACHE_HEADER_VDIR_EXTENSION;

/// Basic information about a cache file entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheFileInfo {
	header_path: PathBuf,
	header_info: apache_cache::Header,
	modified: SystemTime,
	accessed: SystemTime,
}

#[allow(dead_code)]
impl CacheFileInfo {
	#[inline]
	pub fn new(header_entry: &DirEntry) -> Result<Self, IOError> {
		let metadata = header_entry.metadata()?;
		let modified = metadata.modified()?;
		let accessed = metadata.accessed().unwrap_or(modified);
		let header_path = header_entry.path();

		let mut options = OpenOptions::new();
		options.read(true);
		options.custom_flags(libc::O_NOATIME | libc::O_NOCTTY | libc::O_CLOEXEC);
		let mut file = options.open(&header_path)?;
		let header_info = apache_cache::parse(&mut file)?;

		Ok(Self {
			header_path,
			header_info,
			modified,
			accessed,
		})
	}

	/// Path to the `.header` file
	#[inline]
	pub fn header_path(&self) -> &Path {
		&self.header_path
	}

	/// Path to the associated `.data` file
	#[inline]
	pub fn data_path(&self) -> PathBuf {
		let mut data_path = self.header_path.clone();
		data_path.set_extension(&CACHE_DATA_SUFFIX[1..]);
		data_path
	}

	/// Path to the associated `.vary` directory
	#[inline]
	pub fn vary_path(&self) -> PathBuf {
		let mut vary_path = self.header_path.clone();
		vary_path.set_extension(&CACHE_HEADER_VDIR_EXTENSION);
		vary_path
	}

	#[inline]
	pub const fn expires(&self) -> &SystemTime {
		&self.header_info.expiry
	}

	#[inline]
	pub const fn modified(&self) -> &SystemTime {
		&self.modified
	}

	#[inline]
	pub const fn accessed(&self) -> &SystemTime {
		&self.accessed
	}

	#[inline]
	pub const fn is_vary(&self) -> bool {
		matches!(self.header_info.format, apache_cache::Format::Vary)
	}
}

impl PartialOrd<Self> for CacheFileInfo {
	#[inline]
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		Some(self.cmp(other))
	}
}

impl Ord for CacheFileInfo {
	/// Chronological ordering useful for determining what should be deleted from the cache.
	///
	/// First orders by expiry or mtime (whatever is later),
	/// then by mtime or atime (whatever is later), then by mtime.
	///
	/// Tie breaking is done by comparing the path.
	#[inline]
	fn cmp(&self, other: &Self) -> Ordering {
		let cmp1 = max(&self.header_info.expiry, &self.modified)
			.cmp(max(&other.header_info.expiry, &other.modified));
		let cmp2 = max(&self.accessed, &self.modified).cmp(max(&other.accessed, &other.modified));
		let cmp3 = self.modified.cmp(&other.modified);

		match (cmp1, cmp2, cmp3) {
			(Ordering::Equal, Ordering::Equal, Ordering::Equal) => (),
			(Ordering::Equal, Ordering::Equal, result) => return result,
			(Ordering::Equal, result, _) => return result,
			(result, _, _) => return result,
		}

		self.header_path.cmp(&other.header_path)
	}
}
