use std::fmt;
use std::num::{ParseFloatError, ParseIntError};
use std::str::FromStr;
use thiserror::Error;

/// Representation for a user-specified size limit (percentage or absolute)
#[derive(Debug, Clone, Copy)]
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
