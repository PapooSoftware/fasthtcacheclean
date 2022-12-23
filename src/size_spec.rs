// Copyright (c) 2022 Papoo Software & Media GmbH <info@papoo.de>
// SPDX-License-Identifier: LGPL-3.0-or-later

use std::fmt;
use std::num::{ParseFloatError, ParseIntError};
use std::str::FromStr;
use thiserror::Error;

/// Representation for a user-specified size limit (percentage or absolute)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeSpec {
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

/// Error type for parsing a `SizeSpec`
#[derive(Error, Debug)]
pub enum ParseSizeSpecError {
	#[error("expected a positive numeric value with an optional unit")]
	EmptyString,
	#[error("expected a positive numeric value with an optional unit")]
	InvalidInt(#[from] ParseIntError),
	#[error("expected a positive numeric value with an optional unit")]
	InvalidFloat(#[from] ParseFloatError),
	#[error("`{0}` is not a valid unit. Known units are `K`, `Ki`, `M`, `Mi`, `G`, `Gi`, `T`, `Ti`, `%`.")]
	InvalidUnit(char),
}

/// Parsing a string into a `SizeSpec`
impl FromStr for SizeSpec {
	type Err = ParseSizeSpecError;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let first_char = s.chars().next().ok_or(ParseSizeSpecError::EmptyString)?;
		let (last_char, forelast_char) = {
			let mut it = s.chars().rev();
			(
				it.next().ok_or(ParseSizeSpecError::EmptyString)?,
				it.next().unwrap_or(' '),
			)
		};

		if first_char == '-' {
			return Err(s.parse::<u64>().unwrap_err().into());
		}

		match (forelast_char, last_char) {
			(_, '0'..='9') => Ok(SizeSpec::Absolute(s.parse::<u64>()?)),
			('K' | 'k', 'i') => Ok(SizeSpec::Absolute(
				(s[..s.len() - 2].parse::<f64>()? * 1024.0) as u64,
			)),
			('M', 'i') => Ok(SizeSpec::Absolute(
				(s[..s.len() - 2].parse::<f64>()? * 1048576.0) as u64,
			)),
			('G', 'i') => Ok(SizeSpec::Absolute(
				(s[..s.len() - 2].parse::<f64>()? * 1073741824.0) as u64,
			)),
			('T', 'i') => Ok(SizeSpec::Absolute(
				(s[..s.len() - 2].parse::<f64>()? * 1099511627776.0) as u64,
			)),
			(_, 'K' | 'k') => Ok(SizeSpec::Absolute(
				(s[..s.len() - 1].parse::<f64>()? * 1000.0) as u64,
			)),
			(_, 'M') => Ok(SizeSpec::Absolute(
				(s[..s.len() - 1].parse::<f64>()? * 1000000.0) as u64,
			)),
			(_, 'G') => Ok(SizeSpec::Absolute(
				(s[..s.len() - 1].parse::<f64>()? * 1000000000.0) as u64,
			)),
			(_, 'T') => Ok(SizeSpec::Absolute(
				(s[..s.len() - 1].parse::<f64>()? * 1000000000000.0) as u64,
			)),
			(_, '%') => Ok(SizeSpec::Percentage(s[..s.len() - 1].parse::<f64>()?)),
			_ => Err(ParseSizeSpecError::InvalidUnit(last_char)),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Tests string -> `SizeSpec` -> string conversion
	#[test]
	fn test_roundtrip() {
		for string in [
			"0", "100", "50K", "1M", "42G", "1T", "0%", "1%", "99.5%", "101%",
		] {
			let value: SizeSpec = string.parse().unwrap();
			assert_eq!(string, value.to_string());
		}
		assert_eq!("512", "0.512K".parse::<SizeSpec>().unwrap().to_string());
		assert_eq!("5.124K", "5124".parse::<SizeSpec>().unwrap().to_string());
	}

	/// Tests `SizeSpec` parse failure on negative values
	#[test]
	fn test_negative_error() {
		for string in [
			"-0", "-1", "-50K", "-1M", "-42G", "-1T", "-0%", "-1%", "-99.5%", "-101%",
		] {
			assert!(string.parse::<SizeSpec>().is_err());
		}
	}

	/// Tests binary SI units
	#[test]
	fn test_binary_units() {
		for (string, result) in [
			("0Mi", 0),
			("0.5Ki", 512),
			("100Ki", 1024 * 100),
			("1Mi", 1024 * 1024),
			("1Gi", 1024 * 1024 * 1024),
			("1Ti", 1024 * 1024 * 1024 * 1024),
		] {
			let value: SizeSpec = string.parse().unwrap();
			assert_eq!(SizeSpec::Absolute(result), value);
		}
	}

	/// Tests `SizeSpec` parse failure on empty string
	#[test]
	fn test_empty_error() {
		assert!(matches!(
			"".parse::<SizeSpec>().unwrap_err(),
			ParseSizeSpecError::EmptyString
		));
	}

	/// Tests `SizeSpec` parse failure on invalid unit suffixes
	#[test]
	fn test_unit_error() {
		assert!(matches!(
			"1x".parse::<SizeSpec>().unwrap_err(),
			ParseSizeSpecError::InvalidUnit('x')
		));
		assert!(matches!(
			"5.5!".parse::<SizeSpec>().unwrap_err(),
			ParseSizeSpecError::InvalidUnit('!')
		));
	}

	// Tests `SizeSpec::value()` output
	#[test]
	fn test_value() {
		let a = SizeSpec::Absolute(1000);
		assert_eq!(a.value(0), 1000);
		assert_eq!(a.value(9999999), 1000);
		let b = SizeSpec::Percentage(10.0);
		assert_eq!(b.value(10000), 1000);
		assert_eq!(b.value(10), 1);
		assert_eq!(b.value(1), 0);
		let c = SizeSpec::Percentage(0.0);
		assert_eq!(b.value(1), 0);
		assert_eq!(c.value(10000000), 0);
	}
}
