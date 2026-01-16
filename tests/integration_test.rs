//! Integration tests for blkpath crate.

use blkpath::{resolve_device, resolve_device_from_file, DeviceResolveError, ResolveDevice};
use std::fs::File;
use std::path::{Path, PathBuf};

/// Test that resolving the root filesystem device works.
#[test]
fn test_root_device_resolution() {
    let path = Path::new("/");
    let result = path.resolve_device();

    // Root should always be resolvable on Linux
    match result {
        Ok(device) => {
            assert!(
                device.to_string_lossy().starts_with("/dev"),
                "Device path should start with /dev, got: {}",
                device.display()
            );
        }
        Err(DeviceResolveError::DeviceNotFound { major, minor }) => {
            // This can happen in some container environments
            println!("Device not found for root: {}:{}", major, minor);
        }
        Err(e) => {
            panic!("Unexpected error resolving root device: {}", e);
        }
    }
}

/// Test that the convenience function works the same as the trait.
#[test]
fn test_convenience_function_matches_trait() {
    let path = Path::new("/");

    let trait_result = path.resolve_device();
    let fn_result = resolve_device(path);

    match (trait_result, fn_result) {
        (Ok(a), Ok(b)) => assert_eq!(a, b),
        (Err(_), Err(_)) => { /* Both failed, acceptable */ }
        _ => panic!("Results should match"),
    }
}

/// Test that PathBuf works the same as Path.
#[test]
fn test_pathbuf_matches_path() {
    let path = Path::new("/");
    let pathbuf = PathBuf::from("/");

    let path_result = path.resolve_device();
    let pathbuf_result = pathbuf.resolve_device();

    match (path_result, pathbuf_result) {
        (Ok(a), Ok(b)) => assert_eq!(a, b),
        (Err(_), Err(_)) => { /* Both failed, acceptable */ }
        _ => panic!("Results should match"),
    }
}

/// Test that File-based resolution works.
#[test]
fn test_file_resolution() {
    let file = File::open("/").unwrap();
    let result = file.resolve_device();

    match result {
        Ok(device) => {
            assert!(
                device.to_string_lossy().starts_with("/dev"),
                "Device path should start with /dev, got: {}",
                device.display()
            );
        }
        Err(DeviceResolveError::DeviceNotFound { major, minor }) => {
            println!("Device not found for file: {}:{}", major, minor);
        }
        Err(e) => {
            panic!("Unexpected error resolving file device: {}", e);
        }
    }
}

/// Test that File reference resolution works.
#[test]
fn test_file_reference_resolution() {
    let file = File::open("/").unwrap();
    let result = resolve_device_from_file(&file);

    match result {
        Ok(device) => {
            assert!(
                device.to_string_lossy().starts_with("/dev"),
                "Device path should start with /dev, got: {}",
                device.display()
            );
        }
        Err(DeviceResolveError::DeviceNotFound { major, minor }) => {
            println!("Device not found for file ref: {}:{}", major, minor);
        }
        Err(e) => {
            panic!("Unexpected error resolving file device: {}", e);
        }
    }
}

/// Test that non-existent paths return an error.
#[test]
fn test_nonexistent_path_returns_error() {
    let path = Path::new("/nonexistent/path/that/does/not/exist");
    let result = path.resolve_device();

    assert!(result.is_err());
    match result {
        Err(DeviceResolveError::MetadataError(_)) => { /* Expected */ }
        Err(e) => panic!("Unexpected error type: {:?}", e),
        Ok(_) => panic!("Should have failed"),
    }
}

/// Test that consistent results are returned for the same path.
#[test]
fn test_consistent_results() {
    let path = Path::new("/");

    let result1 = path.resolve_device();
    let result2 = path.resolve_device();

    match (result1, result2) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "Same path should return same device"),
        (Err(_), Err(_)) => { /* Both failed, acceptable */ }
        _ => panic!("Inconsistent results"),
    }
}

/// Test that /proc filesystem works (virtual filesystem).
#[test]
fn test_proc_filesystem() {
    if Path::new("/proc").exists() {
        let result = Path::new("/proc").resolve_device();
        // /proc is a virtual filesystem, resolution may or may not succeed
        // depending on the system configuration
        match result {
            Ok(_) | Err(DeviceResolveError::DeviceNotFound { .. }) => { /* Acceptable */ }
            Err(e) => panic!("Unexpected error for /proc: {}", e),
        }
    }
}

/// Test that /tmp directory works.
#[test]
fn test_tmp_directory() {
    if Path::new("/tmp").exists() {
        let result = Path::new("/tmp").resolve_device();
        match result {
            Ok(device) => {
                // /tmp may be on a separate filesystem or on the root filesystem
                assert!(
                    device.to_string_lossy().starts_with("/dev"),
                    "Device path should start with /dev, got: {}",
                    device.display()
                );
            }
            Err(DeviceResolveError::DeviceNotFound { .. }) => {
                // tmpfs may not have a block device
            }
            Err(e) => panic!("Unexpected error for /tmp: {}", e),
        }
    }
}
