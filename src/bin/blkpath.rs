//! blkpath CLI tool
//!
//! Resolves the underlying block device path from a file path.
//!
//! # Usage
//!
//! ```bash
//! blkpath /path/to/file
//! ```

use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

use blkpath::ResolveDevice;

/// Resolve the underlying block device path from a file path.
#[derive(Parser, Debug)]
#[command(name = "blkpath")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to resolve the block device for
    #[arg(value_name = "PATH")]
    path: PathBuf,
}

fn main() -> ExitCode {
    let args = Args::parse();

    match args.path.resolve_device() {
        Ok(device_path) => {
            println!("{}", device_path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}
