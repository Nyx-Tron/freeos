//! File management module for the Neon OS kernel.

use alloc::{
    format,
    string::{String, ToString},
};

pub mod proc;

/// Initialize the filesystem by setting up /proc directories.
pub fn init_filesystem() {
    proc::init_procfs();
}

/// Resolve a path by following all symbolic links to get the final target.
pub fn resolve_symlink_path(path: &str) -> String {
    const MAX_SYMLINK_DEPTH: u32 = 8;
    let mut current_path = path.to_string();

    for _ in 0..MAX_SYMLINK_DEPTH {
        let mut buf = [0u8; 4096];
        match axfs::api::read_link(&current_path, &mut buf) {
            Ok(len) => {
                if let Ok(target) = core::str::from_utf8(&buf[..len]) {
                    current_path = if target.starts_with('/') {
                        target.to_string()
                    } else if let Some(parent_pos) = current_path.rfind('/') {
                        format!("{}/{}", &current_path[..parent_pos], target)
                    } else {
                        target.to_string()
                    };
                } else {
                    break;
                }
            }
            Err(_) => {
                // Not a symlink or doesn't exist, return current path
                return current_path;
            }
        }
    }

    // Too many symlink levels, return original
    path.to_string()
}
