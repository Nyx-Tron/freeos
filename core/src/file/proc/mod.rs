//! File management /proc module for the Neon OS kernel.

use alloc::sync::Arc;

pub mod selfs;

/// Initialize the process filesystem by setting up /proc directories.
pub fn init_procfs() {
    let opts = axfs::fops::OpenOptions::new().set_read(true);
    let procfs = axfs::fops::Directory::open_dir("/proc/self", &opts).unwrap();

    let self_exe = selfs::SelfExe;
    let _ = procfs.add_node("exe", Arc::new(self_exe));
}
