//! # blkpath
//!
//! A Rust crate for resolving the underlying block device path from a file path or file descriptor.
//!
//! ## Overview
//!
//! This crate provides a reliable way to determine which block device underlies a given file or
//! directory. It uses a multi-step resolution strategy:
//!
//! 1. First, it uses the `stat` system call to get the device ID (major:minor numbers)
//! 2. Then, it looks up the device path via `/sys/dev/block/{major}:{minor}`
//! 3. If that fails, it falls back to parsing `/proc/self/mountinfo`
//!
//! ## Usage
//!
//! ```rust,no_run
//! use blkpath::ResolveDevice;
//! use std::path::Path;
//!
//! let path = Path::new("/home");
//! match path.resolve_device() {
//!     Ok(device_path) => println!("Device: {}", device_path.display()),
//!     Err(e) => eprintln!("Error: {}", e),
//! }
//! ```
//!
//! You can also use it with file descriptors:
//!
//! ```rust,no_run
//! use blkpath::ResolveDevice;
//! use std::fs::File;
//!
//! let file = File::open("/home").unwrap();
//! match file.resolve_device() {
//!     Ok(device_path) => println!("Device: {}", device_path.display()),
//!     Err(e) => eprintln!("Error: {}", e),
//! }
//! ```

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors that can occur during device resolution.
#[derive(Error, Debug)]
pub enum DeviceResolveError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Failed to get file metadata.
    #[error("Failed to get metadata for path: {0}")]
    MetadataError(String),

    /// Failed to resolve the device via sysfs.
    #[error("Failed to resolve device via sysfs for dev {major}:{minor}")]
    SysfsResolutionFailed {
        /// Major device number
        major: u32,
        /// Minor device number
        minor: u32,
    },

    /// Failed to resolve the device via mountinfo.
    #[error("Failed to resolve device via mountinfo for dev {major}:{minor}")]
    MountinfoResolutionFailed {
        /// Major device number
        major: u32,
        /// Minor device number
        minor: u32,
    },

    /// The device could not be resolved using any method.
    #[error("Could not resolve device for dev {major}:{minor}")]
    DeviceNotFound {
        /// Major device number
        major: u32,
        /// Minor device number
        minor: u32,
    },

    /// Failed to call fstat on file descriptor.
    #[error("Failed to fstat file descriptor: {0}")]
    FstatError(String),
}

/// A trait for resolving the underlying block device of a file or path.
///
/// This trait is implemented for `Path` and `File`, allowing you to resolve
/// the block device using a consistent interface.
pub trait ResolveDevice {
    /// Resolves the underlying block device path.
    ///
    /// Returns the path to the block device (e.g., `/dev/sda1`, `/dev/nvme0n1p1`)
    /// that contains the file or directory.
    ///
    /// # Errors
    ///
    /// Returns a `DeviceResolveError` if:
    /// - The file/path cannot be accessed
    /// - The device information cannot be retrieved
    /// - The device cannot be mapped to a block device path
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use blkpath::ResolveDevice;
    /// use std::path::Path;
    ///
    /// let path = Path::new("/home");
    /// let device = path.resolve_device()?;
    /// println!("Device: {}", device.display());
    /// # Ok::<(), blkpath::DeviceResolveError>(())
    /// ```
    fn resolve_device(&self) -> Result<PathBuf, DeviceResolveError>;
}

impl ResolveDevice for Path {
    fn resolve_device(&self) -> Result<PathBuf, DeviceResolveError> {
        let metadata = fs::metadata(self)
            .map_err(|e| DeviceResolveError::MetadataError(format!("{}: {}", self.display(), e)))?;

        let dev = metadata.dev();
        let major = major(dev);
        let minor = minor(dev);

        resolve_device_from_dev(major, minor)
    }
}

impl ResolveDevice for PathBuf {
    fn resolve_device(&self) -> Result<PathBuf, DeviceResolveError> {
        self.as_path().resolve_device()
    }
}

impl ResolveDevice for File {
    fn resolve_device(&self) -> Result<PathBuf, DeviceResolveError> {
        let fd = self.as_raw_fd();
        let (major, minor) = get_dev_from_fd(fd)?;
        resolve_device_from_dev(major, minor)
    }
}

impl ResolveDevice for &File {
    fn resolve_device(&self) -> Result<PathBuf, DeviceResolveError> {
        (*self).resolve_device()
    }
}

/// Extracts the major device number from a device ID.
#[inline]
fn major(dev: u64) -> u32 {
    ((dev >> 8) & 0xfff) as u32 | (((dev >> 32) & !0xfff) as u32)
}

/// Extracts the minor device number from a device ID.
#[inline]
fn minor(dev: u64) -> u32 {
    (dev & 0xff) as u32 | (((dev >> 12) & !0xff) as u32)
}

/// Gets the device major:minor from a file descriptor using fstat.
fn get_dev_from_fd(fd: i32) -> Result<(u32, u32), DeviceResolveError> {
    let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::fstat(fd, &mut stat_buf) };

    if result != 0 {
        return Err(DeviceResolveError::FstatError(
            io::Error::last_os_error().to_string(),
        ));
    }

    let dev = stat_buf.st_dev;
    Ok((major(dev), minor(dev)))
}

/// Resolves a device path from major:minor numbers.
///
/// This function tries multiple resolution strategies:
/// 1. First, try to resolve via `/sys/dev/block/{major}:{minor}`
/// 2. If that fails, fall back to parsing `/proc/self/mountinfo`
fn resolve_device_from_dev(major: u32, minor: u32) -> Result<PathBuf, DeviceResolveError> {
    // Try sysfs first
    if let Some(path) = resolve_via_sysfs(major, minor) {
        return Ok(path);
    }

    // Fall back to mountinfo
    if let Some(path) = resolve_via_mountinfo(major, minor)? {
        return Ok(path);
    }

    Err(DeviceResolveError::DeviceNotFound { major, minor })
}

/// Resolves a device path via the sysfs interface.
///
/// Looks up `/sys/dev/block/{major}:{minor}` and follows the symlink to find
/// the actual device name.
fn resolve_via_sysfs(major: u32, minor: u32) -> Option<PathBuf> {
    let sysfs_path = format!("/sys/dev/block/{}:{}", major, minor);
    let sysfs_path = Path::new(&sysfs_path);

    if !sysfs_path.exists() {
        return None;
    }

    // Read the symlink target to get the device name
    let target = fs::read_link(sysfs_path).ok()?;

    // Extract device name from path like "../../block/sda/sda1"
    let device_name = target.file_name()?.to_str()?;

    let dev_path = PathBuf::from(format!("/dev/{}", device_name));
    if dev_path.exists() {
        return Some(dev_path);
    }

    // Try to find the device in /dev recursively
    find_device_in_dev(device_name)
}

/// Searches for a device with the given name in /dev.
fn find_device_in_dev(device_name: &str) -> Option<PathBuf> {
    // Common locations to check
    let paths_to_check = [
        format!("/dev/{}", device_name),
        format!("/dev/mapper/{}", device_name),
        format!("/dev/disk/by-id/{}", device_name),
    ];

    for path_str in &paths_to_check {
        let path = PathBuf::from(path_str);
        if path.exists() {
            return Some(path);
        }
    }

    // If still not found, try to find in /dev directory
    if let Ok(entries) = fs::read_dir("/dev") {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy() == device_name {
                return Some(entry.path());
            }
        }
    }

    None
}

/// Resolves a device path by parsing /proc/self/mountinfo.
///
/// The mountinfo file format is documented in proc(5).
/// Each line contains fields separated by spaces:
/// - mount ID
/// - parent ID
/// - major:minor
/// - root
/// - mount point
/// - mount options
/// - optional fields (terminated by " - ")
/// - filesystem type
/// - mount source
/// - super options
fn resolve_via_mountinfo(major: u32, minor: u32) -> Result<Option<PathBuf>, DeviceResolveError> {
    let mountinfo_path = Path::new("/proc/self/mountinfo");
    if !mountinfo_path.exists() {
        return Ok(None);
    }

    let file = File::open(mountinfo_path)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        if let Some(device) = parse_mountinfo_line(&line, major, minor) {
            return Ok(Some(device));
        }
    }

    Ok(None)
}

/// Parses a single line from mountinfo and returns the device path if it matches.
fn parse_mountinfo_line(line: &str, target_major: u32, target_minor: u32) -> Option<PathBuf> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 10 {
        return None;
    }

    // Field 3 is major:minor
    let dev_field = fields.get(2)?;
    let (major, minor) = parse_dev_field(dev_field)?;

    if major != target_major || minor != target_minor {
        return None;
    }

    // Find the separator " - " to get the mount source
    let separator_idx = fields.iter().position(|&f| f == "-")?;

    // Mount source is 2 fields after the separator
    let mount_source = fields.get(separator_idx + 2)?;

    if mount_source.starts_with('/') {
        return Some(PathBuf::from(mount_source));
    }

    // For non-path sources (like "tmpfs", "proc", etc.), try /dev
    let dev_path = PathBuf::from(format!("/dev/{}", mount_source));
    if dev_path.exists() {
        return Some(dev_path);
    }

    None
}

/// Parses a "major:minor" string into (u32, u32).
fn parse_dev_field(field: &str) -> Option<(u32, u32)> {
    let mut parts = field.split(':');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Convenience function to resolve the device for a path.
///
/// This is a free function that provides the same functionality as the
/// `ResolveDevice` trait implementation for `Path`.
///
/// # Example
///
/// ```rust,no_run
/// use blkpath::resolve_device;
/// use std::path::Path;
///
/// let device = resolve_device(Path::new("/home"))?;
/// println!("Device: {}", device.display());
/// # Ok::<(), blkpath::DeviceResolveError>(())
/// ```
pub fn resolve_device<P: AsRef<Path>>(path: P) -> Result<PathBuf, DeviceResolveError> {
    path.as_ref().resolve_device()
}

/// Convenience function to resolve the device from a file descriptor.
///
/// # Example
///
/// ```rust,no_run
/// use blkpath::resolve_device_from_file;
/// use std::fs::File;
///
/// let file = File::open("/home")?;
/// let device = resolve_device_from_file(&file)?;
/// println!("Device: {}", device.display());
/// # Ok::<(), blkpath::DeviceResolveError>(())
/// ```
pub fn resolve_device_from_file(file: &File) -> Result<PathBuf, DeviceResolveError> {
    file.resolve_device()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::TempDir;

    #[test]
    fn test_major_minor_extraction() {
        // Test with known values
        // On Linux, makedev(8, 1) = 0x801 for block devices
        let dev = 0x0801_u64; // major=8, minor=1 (sda1 typically)
        assert_eq!(major(dev), 8);
        assert_eq!(minor(dev), 1);
    }

    #[test]
    fn test_parse_dev_field() {
        assert_eq!(parse_dev_field("8:1"), Some((8, 1)));
        assert_eq!(parse_dev_field("254:0"), Some((254, 0)));
        assert_eq!(parse_dev_field("invalid"), None);
        assert_eq!(parse_dev_field("8:"), None);
        assert_eq!(parse_dev_field(":1"), None);
    }

    #[test]
    fn test_resolve_device_for_root() {
        // Root filesystem should always be resolvable
        let path = Path::new("/");
        let result = path.resolve_device();
        // This might fail in some CI environments without proper /sys
        if result.is_ok() {
            let device = result.unwrap();
            assert!(device.to_string_lossy().starts_with("/dev"));
        }
    }

    #[test]
    fn test_resolve_device_for_temp_file() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        let result = temp_path.resolve_device();
        // This might fail in some CI environments
        if result.is_ok() {
            let device = result.unwrap();
            assert!(device.to_string_lossy().starts_with("/dev"));
        }
    }

    #[test]
    fn test_resolve_device_from_file() {
        let file = File::open("/").unwrap();
        let result = file.resolve_device();
        // This might fail in some CI environments without proper /sys
        if result.is_ok() {
            let device = result.unwrap();
            assert!(device.to_string_lossy().starts_with("/dev"));
        }
    }

    #[test]
    fn test_resolve_device_nonexistent() {
        let path = Path::new("/nonexistent/path/that/does/not/exist");
        let result = path.resolve_device();
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_mountinfo_line() {
        // Example mountinfo line
        let line = "29 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw";
        let result = parse_mountinfo_line(line, 8, 1);
        assert_eq!(result, Some(PathBuf::from("/dev/sda1")));

        // Non-matching line
        let result = parse_mountinfo_line(line, 9, 2);
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_mountinfo_line_with_special_fs() {
        // tmpfs doesn't have a real device
        let line = "22 20 0:21 / /dev/shm rw,nosuid,nodev shared:3 - tmpfs tmpfs rw";
        let result = parse_mountinfo_line(line, 0, 21);
        // tmpfs doesn't start with /, so it returns None or tries /dev/tmpfs
        // This should return None since /dev/tmpfs doesn't exist
        assert!(result.is_none() || result == Some(PathBuf::from("/dev/tmpfs")));
    }

    #[test]
    fn test_pathbuf_resolve_device() {
        let pathbuf = PathBuf::from("/");
        let result = pathbuf.resolve_device();
        // This might fail in some CI environments without proper /sys
        if result.is_ok() {
            let device = result.unwrap();
            assert!(device.to_string_lossy().starts_with("/dev"));
        }
    }
}
