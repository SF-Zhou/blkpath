# blkpath

[![CI](https://github.com/SF-Zhou/blkpath/actions/workflows/ci.yml/badge.svg)](https://github.com/SF-Zhou/blkpath/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/blkpath.svg)](https://crates.io/crates/blkpath)
[![Documentation](https://docs.rs/blkpath/badge.svg)](https://docs.rs/blkpath)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Resolve the underlying block device path from a file path or file descriptor.

## Overview

This crate provides a reliable way to determine which block device underlies a given file or directory. It uses a multi-step resolution strategy:

1. First, it uses the `stat` system call to get the device ID (major:minor numbers)
2. Then, it looks up the device path via `/sys/dev/block/{major}:{minor}`
3. If that fails, it falls back to parsing `/proc/self/mountinfo`

## Installation

Add `blkpath` to your `Cargo.toml`:

```toml
[dependencies]
blkpath = "0.1"
```

Or install the CLI tool:

```bash
cargo install blkpath
```

## Usage

### Library Usage

Use the `ResolveDevice` trait to resolve device paths from `Path` or `File`:

```rust
use blkpath::ResolveDevice;
use std::path::Path;

fn main() -> Result<(), blkpath::DeviceResolveError> {
    let path = Path::new("/home");
    let device = path.resolve_device()?;
    println!("Device: {}", device.display());
    Ok(())
}
```

You can also use it with file descriptors:

```rust
use blkpath::ResolveDevice;
use std::fs::File;

fn main() -> Result<(), blkpath::DeviceResolveError> {
    let file = File::open("/home")?;
    let device = file.resolve_device()?;
    println!("Device: {}", device.display());
    Ok(())
}
```

Or use the convenience functions:

```rust
use blkpath::{resolve_device, resolve_device_from_file};
use std::path::Path;
use std::fs::File;

fn main() -> Result<(), blkpath::DeviceResolveError> {
    // From path
    let device = resolve_device(Path::new("/home"))?;
    println!("Device: {}", device.display());

    // From file
    let file = File::open("/home")?;
    let device = resolve_device_from_file(&file)?;
    println!("Device: {}", device.display());

    Ok(())
}
```

### CLI Usage

```bash
# Get the block device for a path
blkpath /home
# Output: /dev/sda1

# Get help
blkpath --help
```

## How It Works

The resolution process works as follows:

1. **Stat the file/path**: Get the device ID (major:minor numbers) from file metadata
2. **Sysfs lookup**: Check `/sys/dev/block/{major}:{minor}` for the device symlink
3. **Mountinfo fallback**: If sysfs fails, parse `/proc/self/mountinfo` to find the mount source

This multi-step approach ensures reliability across different Linux configurations and container environments.

## Requirements

- Linux operating system
- Access to `/sys/dev/block/` or `/proc/self/mountinfo`

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
