use crate::{Condition, ParseSizeSpecError, SizeSpec, Stats};
use std::time::Duration;

/// Tests string -> `SizeSpec` -> string conversion
#[test]
fn test_sizespec_roundtrip() {
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
fn test_sizespec_negative_error() {
	for string in [
		"-0", "-1", "-50K", "-1M", "-42G", "-1T", "-0%", "-1%", "-99.5%", "-101%",
	] {
		assert!(string.parse::<SizeSpec>().is_err());
	}
}

/// Tests `SizeSpec` parse failure on empty string
#[test]
fn test_sizespec_empty_error() {
	assert!(matches!(
		"".parse::<SizeSpec>().unwrap_err(),
		ParseSizeSpecError::EmptyString
	));
}

/// Tests `SizeSpec` parse failure on invalid unit suffixes
#[test]
fn test_sizespec_unit_error() {
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
fn test_sizespec_value() {
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

/// Tests `Stats` `Default` implementation
#[test]
fn test_stats_default() {
	let result = <Stats as Default>::default();
	assert_eq!(result.deleted, 0);
	assert_eq!(result.deleted_folders, 0);
	assert_eq!(result.failed, 0);
}

/// Tests `Stats` counting
#[test]
fn test_stats_counting() {
	let mut result = <Stats as Default>::default();
	result.count(Err(()));
	result.count(Err(()));
	result.count_folder(Err(()));
	result.count::<()>(Ok(true));
	result.count::<&'static str>(Ok(true));
	result.count::<i32>(Ok(false));
	result.count_folder::<()>(Ok(true));
	result.count::<bool>(Ok(false));

	assert_eq!(result.deleted, 2);
	assert_eq!(result.deleted_folders, 1);
	assert_eq!(result.failed, 3);
}

/// Tests `Stats` summing
#[test]
fn test_stats_summing() {
	let items = [
		Stats {
			deleted: 50,
			deleted_folders: 3,
			failed: 12,
		},
		Stats {
			deleted: 20,
			deleted_folders: 2,
			failed: 29,
		},
		Stats {
			deleted: 0,
			deleted_folders: 0,
			failed: 0,
		},
		Stats {
			deleted: 0,
			deleted_folders: 0,
			failed: 1,
		},
	];

	let result: Stats = items.into_iter().sum();
	assert_eq!(result.deleted, 70);
	assert_eq!(result.deleted_folders, 5);
	assert_eq!(result.failed, 42);
}

/// Tests `Stats` summing
#[test]
fn test_stats_result_summing() {
	let items: [Result<Stats, ()>; 5] = [
		Ok(Stats {
			deleted: 50,
			deleted_folders: 3,
			failed: 12,
		}),
		Ok(Stats {
			deleted: 20,
			deleted_folders: 2,
			failed: 29,
		}),
		Ok(Stats {
			deleted: 0,
			deleted_folders: 0,
			failed: 0,
		}),
		Err(()),
		Ok(Stats {
			deleted: 0,
			deleted_folders: 0,
			failed: 1,
		}),
	];

	let result: Stats = items.into_iter().sum();
	assert_eq!(result.deleted, 70);
	assert_eq!(result.deleted_folders, 5);
	assert_eq!(result.failed, 43);
}
