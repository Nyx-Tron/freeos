//! Filesystem-related overrides for process information.

use alloc::{
    format,
    string::{String, ToString},
};
use axtask::{TaskExtRef, current};

/// Resolve a path by following all symbolic links to get the final target.
fn resolve_symlink_path(path: &str) -> String {
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

/// Override the weak symbol in axfs to provide the current process executable path.
/// Called by the dynamic symlink generator in ramfs for `/proc/self/exe`.
#[unsafe(no_mangle)]
unsafe fn get_current_process_exe_path() -> String {
    let exe_path = current().task_ext().process_data().exe_path.read().clone();

    resolve_symlink_path(&exe_path)
}
