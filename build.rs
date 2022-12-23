// Copyright (c) 2022 Papoo Software & Media GmbH <info@papoo.de>
// SPDX-License-Identifier: LGPL-3.0-or-later
-->
use clap::CommandFactory;

#[path = "src/cmdargs.rs"]
mod cmdargs;
#[path = "src/size_spec.rs"]
#[allow(dead_code)]
mod size_spec;

fn main() -> std::io::Result<()> {
	let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or_else(|| std::io::ErrorKind::NotFound)?);

	let cmd = cmdargs::Args::command();
	let man = clap_mangen::Man::new(cmd);
	let mut buffer: Vec<u8> = Default::default();
	man.render(&mut buffer)?;

	std::fs::write(out_dir.join("fasthtcacheclean.8"), buffer)?;

	Ok(())
}
