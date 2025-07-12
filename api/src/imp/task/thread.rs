use axerrno::{LinuxError, LinuxResult};
use axprocess::Pid;
use axtask::{TaskExtRef, current};
use num_enum::TryFromPrimitive;
use starry_core::task::{get_process, get_process_group};

pub fn sys_getpid() -> LinuxResult<isize> {
    Ok(axtask::current().task_ext().thread.process().pid() as _)
}

pub fn sys_getppid() -> LinuxResult<isize> {
    Ok(axtask::current()
        .task_ext()
        .thread
        .process()
        .parent()
        .unwrap()
        .pid() as _)
}

pub fn sys_gettid() -> LinuxResult<isize> {
    Ok(axtask::current().id().as_u64() as _)
}

pub fn sys_getpgid(pid: Pid) -> LinuxResult<isize> {
    if pid == 0 {
        Ok(current().task_ext().thread.process().group().pgid() as _)
    } else {
        let process = get_process(pid)?;
        Ok(process.group().pgid() as _)
    }
}

pub fn sys_setpgid(pid: Pid, pgid: Pid) -> LinuxResult<isize> {
    let current_task = current();
    let current_process = current_task.task_ext().thread.process();

    let target_process = if pid == 0 {
        current_process.clone()
    } else {
        get_process(pid)?
    };

    let target_pgid = if pgid == 0 {
        target_process.pid()
    } else {
        pgid
    };

    if target_pgid == target_process.group().pgid() {
        return Ok(0);
    }

    if target_process.pid() != current_process.pid() {
        if target_process
            .parent()
            .is_none_or(|p| p.pid() != current_process.pid())
        {
            return Err(LinuxError::ESRCH);
        }

        if target_process.group().session().sid() != current_process.group().session().sid() {
            return Err(LinuxError::EPERM);
        }
    }

    if target_pgid == target_process.pid() {
        if target_process.create_group().is_none() {
            return Err(LinuxError::EPERM);
        }
    } else {
        let target_group = get_process_group(target_pgid);
        if target_group.is_err() {
            return Err(LinuxError::EPERM);
        }
        let target_group = target_group.unwrap();

        if target_group.session().sid() != target_process.group().session().sid() {
            return Err(LinuxError::EPERM);
        }

        if !target_process.move_to_group(&target_group) {
            return Err(LinuxError::EPERM);
        }
    }

    Ok(0)
}

/// ARCH_PRCTL codes
///
/// It is only avaliable on x86_64, and is not convenient
/// to generate automatically via c_to_rust binding.
#[derive(Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(i32)]
enum ArchPrctlCode {
    /// Set the GS segment base
    SetGs = 0x1001,
    /// Set the FS segment base
    SetFs = 0x1002,
    /// Get the FS segment base
    GetFs = 0x1003,
    /// Get the GS segment base
    GetGs = 0x1004,
    /// The setting of the flag manipulated by ARCH_SET_CPUID
    GetCpuid = 0x1011,
    /// Enable (addr != 0) or disable (addr == 0) the cpuid instruction for the calling thread.
    SetCpuid = 0x1012,
}

/// To set the clear_child_tid field in the task extended data.
///
/// The set_tid_address() always succeeds
pub fn sys_set_tid_address(clear_child_tid: usize) -> LinuxResult<isize> {
    let curr = current();
    curr.task_ext()
        .thread_data()
        .set_clear_child_tid(clear_child_tid);
    Ok(curr.id().as_u64() as isize)
}

#[cfg(target_arch = "x86_64")]
pub fn sys_arch_prctl(
    tf: &mut axhal::arch::TrapFrame,
    code: i32,
    addr: usize,
) -> LinuxResult<isize> {
    use crate::ptr::UserPtr;

    let code = ArchPrctlCode::try_from(code).map_err(|_| axerrno::LinuxError::EINVAL)?;
    debug!("sys_arch_prctl: code = {:?}, addr = {:#x}", code, addr);

    match code {
        // According to Linux implementation, SetFs & SetGs does not return
        // error at all
        ArchPrctlCode::GetFs => {
            *UserPtr::from(addr).get_as_mut()? = tf.tls();
            Ok(0)
        }
        ArchPrctlCode::SetFs => {
            tf.set_tls(addr);
            Ok(0)
        }
        ArchPrctlCode::GetGs => {
            *UserPtr::from(addr).get_as_mut()? =
                unsafe { x86::msr::rdmsr(x86::msr::IA32_KERNEL_GSBASE) };
            Ok(0)
        }
        ArchPrctlCode::SetGs => {
            unsafe {
                x86::msr::wrmsr(x86::msr::IA32_KERNEL_GSBASE, addr as _);
            }
            Ok(0)
        }
        ArchPrctlCode::GetCpuid => Ok(0),
        ArchPrctlCode::SetCpuid => Err(axerrno::LinuxError::ENODEV),
    }
}
