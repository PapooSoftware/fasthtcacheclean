// Copyright (c) 2023 Christoph Grenz (Papoo Software & Media GmbH) <info@papoo.de>
// SPDX-License-Identifier: LGPL-3.0-or-later

use std::fmt;
use std::num::{NonZeroUsize, ParseIntError};
use std::str::FromStr;
use thiserror::Error;

/// Representation for a job count specification
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum JobCount {
	#[default]
	Auto,
	Fixed(NonZeroUsize)
}

impl fmt::Display for JobCount {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Auto => f.write_str("auto"),
			Self::Fixed(n) => write!(f, "{}", n),
		}
	}
}

impl JobCount {
	/// Returns the fixed job count or computes the 'auto' job count from closure
	#[inline]
	pub fn unwrap_or_else<F: FnOnce() -> usize>(self, f: F) -> usize {
		match self {
			Self::Auto => f(),
			Self::Fixed(n) => n.into()
		}
	}
}

/// Error type for parsing a `JobCount`
#[derive(Error, Debug)]
#[error("expected a positive integer or 'auto'")]
pub struct ParseJobCountError(#[from] ParseIntError);

/// Parsing a string into a `JobCount`
impl FromStr for JobCount {
	type Err = ParseJobCountError;
	#[inline]
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		if s.eq_ignore_ascii_case("auto") {
			Ok(Self::Auto)
		} else {
			Ok(Self::Fixed(s.parse::<NonZeroUsize>().map_err(ParseJobCountError::from)?))
		}
	}
}
