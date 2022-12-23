# fasthtcacheclean - A Faster Apache Disk Cache Cleaner

A replacement for `apache-htcacheclean`, which is faster in cleaning the disk cache.

It is primarily designed for disk caches on a separate partition or RAM disk.

Only unixoid operating systems like Linux are supported. Daemon mode is currently not supported. It has to be run via a systemd timer or a cronjob.

## Building and Installation

To build it, `cargo` is required:

```sh
	sudo apt install cargo
```

Then `fasthtcacheclean` can be built with

```sh
	cargo build --release
```

The result can be found in `target/release/fasthtcacheclean`.

To install it:

```
cp target/release/fasthtcacheclean /usr/local/bin/
cp fasthtcacheclean.{service,timer} /etc/systemd/system/
systemctl daemon-reload
systemctl stop apache-htcacheclean
systemctl disable apache-htcacheclean
systemctl enable fasthtcacheclean.timer
systemctl start fasthtcacheclean.timer
```

## Functionality

It works similarily to  `apache-htcacheclean` in non-daemon mode with some optimizations and parallelization:

1. First it checks if the set limit was exceeded or is nearly reached. Starting with 90 % of the limit, the first files are deleted.
2. Old temporary files in the cache main directory are deleted (if modified more than 15 minutes ago).
3. Then the directory tree is scanned (by default using CPUs/2 threads in parallel).
   Old empty directories and orphaned `.data` files are deleted directly.
   Cache entries are sorted by expiry date, access date and modification date into a priority queue.
   To keep the RAM usage low, only the oldest up to 1'000'000 entries are kept for step 4.
4. The found cache entries are deleted until disk usage is 99.0 to 99.5 % of the limit.
   If more than 105 % of the limit were used, header file deletion happens more agressively.


<!--
 Copyright (c) 2022 Papoo Software & Media GmbH <info@papoo.de>
 SPDX-License-Identifier: LGPL-3.0-or-later
-->
