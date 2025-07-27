//! System V shared memory implementation for Neon-OS.
//!
//! This module provides System V shared memory IPC support including:
//! - Shared memory segment management with automatic cleanup
//! - Per-process shared memory tracking  
//! - Secure ID allocation with collision detection
//! - Linux-compatible permissions and error handling

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use axalloc::global_allocator;
use axerrno::{AxError, AxResult};
use axhal::mem::{PAGE_SIZE_4K, virt_to_phys};
use axsync::Mutex;
use axtask::{TaskExtRef, current};
use core::sync::atomic::AtomicBool;
use lazy_static::lazy_static;
use memory_addr::{PhysAddr, VirtAddr, align_up_4k};

/// Shared memory segment identifier.
pub type ShmId = i32;

/// Shared memory key.
pub type ShmKey = i32;

/// IPC_PRIVATE key value.
pub const IPC_PRIVATE: ShmKey = 0;

lazy_static! {
    /// Global shared memory manager instance.
    static ref SHM_MANAGER: Mutex<ShmManager> = Mutex::new(ShmManager::new());
}

/// Shared memory segment data structure (shmid_ds)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ShmidDs {
    /// IPC permissions
    pub shm_perm: IpcPerm,
    /// Size of segment in bytes
    pub shm_segsz: usize,
    /// Last attach time
    pub shm_atime: i64,
    /// Last detach time
    pub shm_dtime: i64,
    /// Last change time
    pub shm_ctime: i64,
    /// Creator PID
    pub shm_cpid: i32,
    /// Last operator PID
    pub shm_lpid: i32,
    /// Number of current attaches
    pub shm_nattch: u64,
    /// Unused fields for future expansion
    pub shm_unused: [u32; 4],
}

/// IPC permission structure
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IpcPerm {
    /// Key supplied to shmget()
    pub key: i32,
    /// Effective UID of owner
    pub uid: u32,
    /// Effective GID of owner
    pub gid: u32,
    /// Effective UID of creator
    pub cuid: u32,
    /// Effective GID of creator
    pub cgid: u32,
    /// Permissions
    pub mode: u32,
    /// Sequence number
    pub seq: u32,
    /// Unused
    pub _unused1: [u32; 5],
}

/// Shared memory segment.
#[derive(Debug)]
pub struct ShmSegment {
    /// Shared memory segment identifier.
    pub id: ShmId,
    /// Physical address of the segment.
    pub paddr: PhysAddr,
    /// Size of the segment in bytes.
    pub size: usize,
    /// Standard Linux shmid_ds structure (protected by mutex).
    pub shmid_ds: Mutex<ShmidDs>,
    /// Whether this segment is marked for deletion.
    pub marked_for_deletion: AtomicBool,
}

impl ShmSegment {
    /// Creates a new shared memory segment.
    pub fn new(id: ShmId, key: ShmKey, size: usize, mode: u16) -> AxResult<Self> {
        let aligned_size = align_up_4k(size);

        let vaddr = global_allocator()
            .alloc_pages(aligned_size / PAGE_SIZE_4K, PAGE_SIZE_4K)
            .map_err(|_| AxError::NoMemory)?;

        let paddr = virt_to_phys(vaddr.into());
        let current_time = axhal::time::wall_time().as_secs();
        let creator_pid = current().task_ext().thread.process().pid() as i32;

        let ipc_perm = IpcPerm {
            key,
            uid: 0,
            gid: 0,
            cuid: 0,
            cgid: 0,
            mode: mode as u32,
            seq: 0,
            _unused1: [0; 5],
        };

        let shmid_ds = ShmidDs {
            shm_perm: ipc_perm,
            shm_segsz: size,
            shm_atime: 0,
            shm_dtime: 0,
            shm_ctime: current_time as i64,
            shm_cpid: creator_pid,
            shm_lpid: 0,
            shm_nattch: 0,
            shm_unused: [0; 4],
        };

        Ok(Self {
            id,
            paddr,
            size: aligned_size,
            shmid_ds: Mutex::new(shmid_ds),
            marked_for_deletion: AtomicBool::new(false),
        })
    }

    /// Increments the attachment count for this segment.
    pub fn inc_attach(&self) {
        let mut ds = self.shmid_ds.lock();
        ds.shm_nattch += 1;
        ds.shm_atime = axhal::time::wall_time().as_secs() as i64;
    }

    /// Decrements the attachment count for this segment.
    pub fn dec_attach(&self) {
        let mut ds = self.shmid_ds.lock();
        if ds.shm_nattch > 0 {
            ds.shm_nattch -= 1;
            if ds.shm_nattch == 0 {
                ds.shm_dtime = axhal::time::wall_time().as_secs() as i64;
            }
        }
    }

    /// Gets the current attachment count for this segment.
    pub fn get_attach_count(&self) -> usize {
        self.shmid_ds.lock().shm_nattch as usize
    }

    /// Updates the last process ID that performed an operation.
    pub fn set_last_pid(&self, pid: i32) {
        self.shmid_ds.lock().shm_lpid = pid;
    }

    /// Checks if the given user has the required permissions for this segment.
    pub fn check_permissions(&self, uid: u32, gid: u32, access: u16) -> bool {
        let ds = self.shmid_ds.lock();
        let mode = ds.shm_perm.mode;

        if uid == ds.shm_perm.uid {
            return (mode & ((access as u32) << 6)) == ((access as u32) << 6);
        }

        if gid == ds.shm_perm.gid {
            return (mode & ((access as u32) << 3)) == ((access as u32) << 3);
        }

        (mode & (access as u32)) == (access as u32)
    }

    /// Validates that the segment is in a consistent state.
    pub fn validate(&self) -> bool {
        let ds = self.shmid_ds.lock();

        if ds.shm_nattch > 65536 {
            return false;
        }

        if self.size < ds.shm_segsz || self.size < align_up_4k(ds.shm_segsz) {
            return false;
        }

        if ds.shm_segsz == 0 || ds.shm_segsz > (1usize << 30) {
            return false;
        }

        if self.size == 0 || self.size > (1usize << 30) {
            return false;
        }

        if self.paddr.as_usize() == 0 {
            return false;
        }

        true
    }

    /// Gets a copy of the shmid_ds structure for IPC_STAT.
    pub fn get_stat(&self) -> ShmidDs {
        *self.shmid_ds.lock()
    }

    /// Updates permissions from user space (for IPC_SET).
    pub fn set_perm(&self, uid: u32, gid: u32, mode: u32) {
        let mut ds = self.shmid_ds.lock();
        ds.shm_perm.uid = uid;
        ds.shm_perm.gid = gid;
        ds.shm_perm.mode = mode;
        ds.shm_ctime = axhal::time::wall_time().as_secs() as i64;
    }
}

impl Drop for ShmSegment {
    fn drop(&mut self) {
        let vaddr = axhal::mem::phys_to_virt(self.paddr);
        global_allocator().dealloc_pages(vaddr.as_usize(), self.size / PAGE_SIZE_4K);
    }
}

/// Global shared memory manager.
pub struct ShmManager {
    /// Map from segment ID to segment.
    segments: BTreeMap<ShmId, Arc<ShmSegment>>,
    /// Map from key to segment ID.
    key_to_id: BTreeMap<ShmKey, ShmId>,
    /// Next segment ID to allocate.
    next_id: ShmId,
}

impl ShmManager {
    /// Creates a new shared memory manager.
    pub fn new() -> Self {
        Self {
            segments: BTreeMap::new(),
            key_to_id: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Allocates a new segment ID with collision detection.
    fn alloc_id(&mut self) -> AxResult<ShmId> {
        const MAX_ATTEMPTS: usize = 1000;
        let mut attempts = 0;

        if self.segments.len() >= (i32::MAX as usize / 2) {
            return Err(AxError::NoMemory);
        }

        loop {
            let id = self.next_id;

            self.next_id = self.next_id.wrapping_add(1);
            if self.next_id <= 0 {
                self.next_id = 1;
            }

            if !self.segments.contains_key(&id) {
                return Ok(id);
            }

            attempts += 1;
            if attempts >= MAX_ATTEMPTS {
                return Err(AxError::NoMemory);
            }
        }
    }

    /// Creates or gets a shared memory segment.
    pub fn get_or_create(
        &mut self,
        key: ShmKey,
        size: usize,
        flags: i32,
    ) -> AxResult<Arc<ShmSegment>> {
        let create_flag = flags & 0o01000;
        let excl_flag = flags & 0o02000;
        let mode = (flags & 0o777) as u16;

        if key == IPC_PRIVATE {
            let id = self.alloc_id()?;
            let segment = Arc::new(ShmSegment::new(id, key, size, mode)?);
            self.segments.insert(id, segment.clone());
            return Ok(segment);
        }

        if let Some(&existing_id) = self.key_to_id.get(&key) {
            if excl_flag != 0 {
                return Err(AxError::AlreadyExists);
            }

            if let Some(segment) = self.segments.get(&existing_id) {
                if segment
                    .marked_for_deletion
                    .load(core::sync::atomic::Ordering::SeqCst)
                {
                    return Err(AxError::NotFound);
                }
                return Ok(segment.clone());
            } else {
                self.key_to_id.remove(&key);
                return Err(AxError::NotFound);
            }
        }

        if create_flag != 0 {
            let id = self.alloc_id()?;
            let segment = Arc::new(ShmSegment::new(id, key, size, mode)?);
            self.segments.insert(id, segment.clone());
            self.key_to_id.insert(key, id);
            Ok(segment)
        } else {
            Err(AxError::NotFound)
        }
    }

    /// Gets a shared memory segment by ID.
    pub fn get_by_id(&self, id: ShmId) -> AxResult<Arc<ShmSegment>> {
        self.segments.get(&id).cloned().ok_or(AxError::NotFound)
    }

    /// Removes a shared memory segment.
    pub fn remove(&mut self, id: ShmId) -> AxResult<()> {
        if let Some(segment) = self.segments.remove(&id) {
            let key = segment.shmid_ds.lock().shm_perm.key;
            if key != IPC_PRIVATE {
                self.key_to_id.remove(&key);
            }
            Ok(())
        } else {
            Err(AxError::NotFound)
        }
    }

    /// Lists all segments (for debugging/info purposes).
    pub fn list_segments(&self) -> impl Iterator<Item = &Arc<ShmSegment>> {
        self.segments.values()
    }
}

/// Shared memory attached regions per process.
#[derive(Debug)]
pub struct ShmAttach {
    /// Segment ID.
    pub id: ShmId,
    /// Virtual address where attached.
    pub addr: VirtAddr,
    /// Segment reference.
    pub segment: Arc<ShmSegment>,
}

/// Per-process shared memory tracking.
#[derive(Debug, Default)]
pub struct ProcessShmData {
    /// Attached shared memory segments.
    pub attached: BTreeMap<VirtAddr, ShmAttach>,
}

impl ProcessShmData {
    /// Creates a new [`ProcessShmData`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Attaches a shared memory segment.
    pub fn attach(&mut self, id: ShmId, addr: VirtAddr, segment: Arc<ShmSegment>) {
        let attach = ShmAttach { id, addr, segment };
        self.attached.insert(addr, attach);
    }

    /// Detaches a shared memory segment.
    pub fn detach(&mut self, addr: VirtAddr) -> Option<ShmAttach> {
        self.attached.remove(&addr)
    }

    /// Finds attached segment by address.
    pub fn find_by_addr(&self, addr: VirtAddr) -> Option<&ShmAttach> {
        self.attached.get(&addr)
    }
}

/// Gets the global shared memory manager.
pub fn shm_manager() -> &'static Mutex<ShmManager> {
    &SHM_MANAGER
}
