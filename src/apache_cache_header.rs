use libc::c_int;
use std::io;
use std::mem::size_of;
use std::ops::Add;
use std::time::{Duration, SystemTime};
use thiserror::Error;

const VARY_FORMAT_VERSION: u32 = 5;
const DISK_FORMAT_VERSION: u32 = 6;

/// Error type for when a file format could not be recognized.
///
/// Will usually be wrapped in a `io::Error` of type `InvalidData`.
#[derive(Error, Debug, Clone, Copy)]
#[error("unknown apache cache header format `{0}`")]
pub struct FormatError(u32);

/// Read the expiration time from an Apache cache header file
pub fn read_expiration_time(mut f: impl io::Read) -> Result<SystemTime, io::Error> {
	let mut buffer = [0u8; 4];
	f.read_exact(&mut buffer)?;
	let format = u32::from_ne_bytes(buffer);

	let microseconds = if format == DISK_FORMAT_VERSION {
		let mut buffer = [0u8; size_of::<c_int>() + size_of::<usize>() * 2 + 8 * 2];
		f.read_exact(&mut buffer)?;
		u64::from_ne_bytes(buffer[buffer.len() - 8..].try_into().unwrap())
	} else if format == VARY_FORMAT_VERSION {
		let mut buffer = [0u8; 8];
		f.read_exact(&mut buffer)?;
		u64::from_ne_bytes(buffer)
	} else {
		return Err(io::Error::new(
			io::ErrorKind::InvalidData,
			FormatError(format),
		));
	};

	Ok(SystemTime::UNIX_EPOCH.add(Duration::from_micros(microseconds)))
}
