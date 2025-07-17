//! System V shared memory system calls.

use alloc::sync::Arc;
use axerrno::{LinuxError, LinuxResult};
use axhal::paging::{MappingFlags, PageSize};
use axtask::{TaskExtRef, current};
use memory_addr::VirtAddr;
use starry_core::shm::{ShmId, ShmKey, ShmSegment, ShmidDs, shm_manager};

use crate::ptr::UserPtr;

const IPC_RMID: i32 = 0;
const IPC_STAT: i32 = 2;
const IPC_SET: i32 = 1;

const SHM_RND: i32 = 0o020000;
const SHM_RDONLY: i32 = 0o010000;

const MAX_SHM_SIZE: usize = 1 << 30; // 1GB

/// Validates segment consistency and permissions.
fn validate_segment(segment: &Arc<ShmSegment>, shmflg: i32) -> LinuxResult<()> {
    if segment
        .marked_for_deletion
        .load(core::sync::atomic::Ordering::SeqCst)
    {
        return Err(LinuxError::EIDRM);
    }
    if !segment.validate() {
        return Err(LinuxError::EINVAL);
    }
    let access = if (shmflg & SHM_RDONLY) != 0 { 0o4 } else { 0o6 };
    if !segment.check_permissions(0, 0, access) {
        return Err(LinuxError::EACCES);
    }
    Ok(())
}

/// shmget system call - get shared memory segment.
pub fn sys_shmget(key: ShmKey, size: usize, flags: i32) -> LinuxResult<isize> {
    info!("sys_shmget: key={}, size={}, flags={:#x}", key, size, flags);
    if size > MAX_SHM_SIZE || (size == 0 && key != starry_core::shm::IPC_PRIVATE) {
        return Err(LinuxError::EINVAL);
    }
    let segment = shm_manager().lock().get_or_create(key, size, flags)?;
    Ok(segment.id as isize)
}

/// shmat system call - attach shared memory segment.
pub fn sys_shmat(shmid: ShmId, shmaddr: usize, shmflg: i32) -> LinuxResult<isize> {
    info!(
        "sys_shmat: shmid={}, shmaddr={:#x}, shmflg={:#x}",
        shmid, shmaddr, shmflg
    );
    if shmid < 0 {
        return Err(LinuxError::EINVAL);
    }
    if (shmflg & SHM_RND) == 0 && shmaddr != 0 && (shmaddr & (axhal::mem::PAGE_SIZE_4K - 1)) != 0 {
        return Err(LinuxError::EINVAL);
    }
    let curr = current();
    let process_data = curr.task_ext().process_data();
    let mut aspace = process_data.aspace.lock();
    let segment = {
        let manager = shm_manager().lock();
        let segment = manager.get_by_id(shmid)?;
        validate_segment(&segment, shmflg)?;
        segment.inc_attach();
        segment
    };
    let size = segment.size;
    let vaddr = if (shmflg & SHM_RND) != 0 {
        if shmaddr == 0 {
            return Err(LinuxError::EINVAL);
        }
        VirtAddr::from(shmaddr & !(axhal::mem::PAGE_SIZE_4K - 1))
    } else if aspace.contains_range(VirtAddr::from(shmaddr), size) {
        return Err(LinuxError::EINVAL);
    } else {
        aspace
            .find_free_area(
                VirtAddr::from(shmaddr),
                size,
                memory_addr::VirtAddrRange::new(aspace.base(), aspace.end()),
                PageSize::Size4K,
            )
            .or_else(|| {
                aspace.find_free_area(
                    aspace.base(),
                    size,
                    memory_addr::VirtAddrRange::new(aspace.base(), aspace.end()),
                    PageSize::Size4K,
                )
            })
            .ok_or(LinuxError::ENOMEM)?
    };
    let mut flags = MappingFlags::USER | MappingFlags::READ;
    if (shmflg & SHM_RDONLY) == 0 {
        flags |= MappingFlags::WRITE;
    }
    let map_result = aspace.map_linear(vaddr, segment.paddr, size, flags, PageSize::Size4K);
    if let Err(e) = map_result {
        segment.dec_attach();
        return Err(LinuxError::from(e));
    }
    let mut shm_data = process_data.shm_data.lock();
    shm_data.attach(shmid, vaddr, segment);
    Ok(vaddr.as_usize() as isize)
}

/// shmdt system call - detach shared memory segment.
pub fn sys_shmdt(shmaddr: usize) -> LinuxResult<isize> {
    info!("sys_shmdt: shmaddr={:#x}", shmaddr);
    let curr = current();
    let process_data = curr.task_ext().process_data();
    let mut aspace = process_data.aspace.lock();
    let mut shm_data = process_data.shm_data.lock();
    let vaddr = VirtAddr::from(shmaddr);
    let attach = shm_data.detach(vaddr).ok_or(LinuxError::EINVAL)?;
    aspace.unmap(vaddr, attach.segment.size)?;
    attach.segment.dec_attach();
    let pid = curr.task_ext().thread.process().pid() as i32;
    attach.segment.set_last_pid(pid);
    let mut manager = shm_manager().lock();
    if attach
        .segment
        .marked_for_deletion
        .load(core::sync::atomic::Ordering::SeqCst)
        && attach.segment.get_attach_count() == 0
    {
        manager.remove(attach.id)?;
    }
    Ok(0)
}

/// shmctl system call - control shared memory segment.
pub fn sys_shmctl(shmid: ShmId, cmd: i32, buf: UserPtr<ShmidDs>) -> LinuxResult<isize> {
    info!("sys_shmctl: shmid={}, cmd={}", shmid, cmd);
    let mut manager = shm_manager().lock();
    let segment = manager.get_by_id(shmid)?;
    match cmd {
        IPC_RMID => {
            segment
                .marked_for_deletion
                .store(true, core::sync::atomic::Ordering::SeqCst);
            if segment.get_attach_count() == 0 {
                manager.remove(shmid)?;
            }
            Ok(0)
        }
        IPC_STAT => {
            if buf.is_null() {
                return Err(LinuxError::EFAULT);
            }
            let stat = segment.get_stat();
            let user_stat = buf.get_as_mut()?;
            *user_stat = stat;
            Ok(0)
        }
        IPC_SET => {
            if buf.is_null() {
                return Err(LinuxError::EFAULT);
            }
            let user_stat = buf.get_as_mut()?;
            segment.set_perm(
                user_stat.shm_perm.uid,
                user_stat.shm_perm.gid,
                user_stat.shm_perm.mode,
            );
            Ok(0)
        }
        _ => {
            warn!("sys_shmctl: unsupported command {}", cmd);
            Err(LinuxError::EINVAL)
        }
    }
}
