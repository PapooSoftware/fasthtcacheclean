// Copyright (c) 2022 Christoph Grenz (Papoo Software & Media GmbH) <info@papoo.de>
// SPDX-License-Identifier: LGPL-3.0-or-later
//
use libc::c_int;
use std::io;
use std::mem::size_of;
use std::ops::Add;
use std::time::{Duration, SystemTime};
use thiserror::Error;

#[repr(u32)]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Format {
	Vary = 5,
	Disk = 6,
}

impl TryFrom<u32> for Format {
	type Error = FormatError;

	#[inline]
	fn try_from(value: u32) -> Result<Self, Self::Error> {
		match value {
			5 => Ok(Self::Vary),
			6 => Ok(Self::Disk),
			_ => Err(FormatError(value)),
		}
	}
}

/// Basic Apache cache header file information
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Header {
	pub format: Format,
	pub expiry: SystemTime,
}

/// Error type for when a file format could not be recognized.
///
/// Will usually be wrapped in a `io::Error` of type `InvalidData`.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
#[error("unknown apache cache header format `{0}`")]
pub struct FormatError(u32);

impl From<FormatError> for io::Error {
	#[inline]
	fn from(error: FormatError) -> Self {
		io::Error::new(io::ErrorKind::InvalidData, error)
	}
}

/// Read the format and expiration time from an Apache cache header file
pub fn parse(mut f: impl io::Read) -> Result<Header, io::Error> {
	let mut buffer = [0u8; 4];
	f.read_exact(&mut buffer)?;
	let format = Format::try_from(u32::from_ne_bytes(buffer))?;

	let microseconds = match format {
		Format::Disk => {
			let mut buffer = [0u8; size_of::<c_int>() + size_of::<usize>() * 2 + 8 * 2];
			f.read_exact(&mut buffer)?;
			u64::from_ne_bytes(buffer[buffer.len() - 8..].try_into().unwrap())
		}
		Format::Vary => {
			let mut buffer = [0u8; 8];
			f.read_exact(&mut buffer)?;
			u64::from_ne_bytes(buffer)
		}
	};

	Ok(Header {
		format,
		expiry: SystemTime::UNIX_EPOCH.add(Duration::from_micros(microseconds)),
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs::File;

	#[test]
	fn test_formats() {
		assert_eq!(Format::try_from(65536), Err(FormatError(65536)));
		assert_eq!(Format::try_from(5), Ok(Format::Vary));
		assert_eq!(Format::try_from(6), Ok(Format::Disk));
	}

	#[test]
	fn test_invalid_data() {
		let error = parse([255u8, 255, 255, 255, 0, 0, 0, 0].as_ref()).unwrap_err();

		assert_eq!(error.kind(), io::ErrorKind::InvalidData);
		let inner_error: FormatError = *error.get_ref().unwrap().downcast_ref().unwrap();
		assert_eq!(
			inner_error,
			FormatError(u32::from_ne_bytes([255, 255, 255, 255]))
		);
	}

	#[test]
	fn test_vary_header() {
		let file = File::open("testcases/vary.header").unwrap();
		let header = parse(file).unwrap();

		assert_eq!(header.format, Format::Vary);
		assert_eq!(
			header
				.expiry
				.duration_since(SystemTime::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			1656536974
		);
	}

	#[test]
	fn test_disk_header() {
		let file = File::open("testcases/disk.header").unwrap();
		let header = parse(file).unwrap();

		assert_eq!(header.format, Format::Disk);
		assert_eq!(
			header
				.expiry
				.duration_since(SystemTime::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			1656657076
		);
	}
}
