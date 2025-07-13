use core::ffi::c_char;

use alloc::{string::ToString, vec::Vec};
use axerrno::{LinuxError, LinuxResult};
use axhal::arch::TrapFrame;
use axtask::{TaskExtRef, current};
use starry_core::mm::{load_user_app, map_trampoline};
use xmas_elf::ElfFile;

use crate::ptr::UserConstPtr;

/// Validate if the file is a valid executable format
fn validate_executable(data: &[u8]) -> LinuxResult<()> {
    if data.starts_with(b"#!") || ElfFile::new(data).is_ok() {
        Ok(())
    } else {
        Err(LinuxError::ENOEXEC)
    }
}

pub fn sys_execve(
    tf: &mut TrapFrame,
    path: UserConstPtr<c_char>,
    argv: UserConstPtr<UserConstPtr<c_char>>,
    envp: UserConstPtr<UserConstPtr<c_char>>,
) -> LinuxResult<isize> {
    let path = path.get_as_str()?.to_string();

    let args = argv
        .get_as_null_terminated()?
        .iter()
        .map(|arg| arg.get_as_str().map(Into::into))
        .collect::<Result<Vec<_>, _>>()?;
    let envs = envp
        .get_as_null_terminated()?
        .iter()
        .map(|env| env.get_as_str().map(Into::into))
        .collect::<Result<Vec<_>, _>>()?;

    info!(
        "sys_execve: path: {:?}, args: {:?}, envs: {:?}",
        path, args, envs
    );

    let curr = current();
    let curr_ext = curr.task_ext();

    if curr_ext.thread.process().threads().len() > 1 {
        // TODO: handle multi-thread case
        error!("sys_execve: multi-thread not supported");
        return Err(LinuxError::EAGAIN);
    }

    // Validate the executable without modifying the address space
    let file_data = axfs::api::read(&path).map_err(|_| LinuxError::ENOENT)?;
    validate_executable(&file_data)?;

    // Proceed with execve
    let mut aspace = curr_ext.process_data().aspace.lock();
    aspace.unmap_user_areas()?;
    map_trampoline(&mut aspace)?;
    axhal::arch::flush_tlb(None);

    let (entry_point, user_stack_base) =
        load_user_app(&mut aspace, &path, &args, &envs).map_err(|e| {
            error!("Failed to load app {}: {:?}", path, e);
            LinuxError::ENOENT
        })?;
    drop(aspace);

    // Set process name and executable path
    let name = path
        .rsplit_once('/')
        .map_or(path.as_str(), |(_, name)| name);
    curr.set_name(name);
    *curr_ext.process_data().exe_path.write() = path;

    // TODO: fd close-on-exec

    tf.set_ip(entry_point.as_usize());
    tf.set_sp(user_stack_base.as_usize());
    Ok(0)
}
