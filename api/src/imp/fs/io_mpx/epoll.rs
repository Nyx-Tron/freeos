//! epoll system calls

use core::{ffi::c_int, time::Duration};

use crate::file::{FileLike, Kstat, add_file_like, get_file_like};
use crate::ptr::UserPtr;
use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use axerrno::{LinuxError, LinuxResult};
use axhal::time::wall_time;
use linux_raw_sys::general::{
    EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD, EPOLLERR, EPOLLIN, EPOLLOUT, sigset_t,
};
use spin::Mutex;

/// Structure representing epoll_event for user space
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EpollEvent {
    pub events: u32,
    pub data: u64,
}

unsafe impl Send for EpollEvent {}
unsafe impl Sync for EpollEvent {}

/// Epoll instance structure
pub struct EpollInstance {
    events: Mutex<BTreeMap<usize, EpollEvent>>,
}

impl EpollInstance {
    fn new(_flags: usize) -> Self {
        Self {
            events: Mutex::new(BTreeMap::new()),
        }
    }

    fn from_fd(fd: c_int) -> LinuxResult<Arc<Self>> {
        get_file_like(fd)?
            .into_any()
            .downcast::<EpollInstance>()
            .map_err(|_| LinuxError::EINVAL)
    }

    fn control(&self, op: usize, fd: usize, event: &EpollEvent) -> LinuxResult<usize> {
        // Verify that the fd exists
        get_file_like(fd as c_int)?;

        let mut events = self.events.lock();
        match op as u32 {
            EPOLL_CTL_ADD => {
                if events.contains_key(&fd) {
                    return Err(LinuxError::EEXIST);
                }
                events.insert(fd, *event);
            }
            EPOLL_CTL_MOD => {
                if !events.contains_key(&fd) {
                    return Err(LinuxError::ENOENT);
                }
                events.insert(fd, *event);
            }
            EPOLL_CTL_DEL => {
                if !events.contains_key(&fd) {
                    return Err(LinuxError::ENOENT);
                }
                events.remove(&fd);
            }
            _ => return Err(LinuxError::EINVAL),
        }
        Ok(0)
    }

    fn poll_all(&self, events: &mut [EpollEvent]) -> LinuxResult<usize> {
        let ready_list = self.events.lock();
        let mut events_num = 0;

        for (&infd, ev) in ready_list.iter() {
            if events_num >= events.len() {
                break;
            }

            match get_file_like(infd as c_int).and_then(|f| f.poll()) {
                Err(_) => {
                    if (ev.events & EPOLLERR) != 0 {
                        events[events_num].events = EPOLLERR;
                        events[events_num].data = ev.data;
                        events_num += 1;
                    }
                }
                Ok(state) => {
                    if state.readable && (ev.events & EPOLLIN != 0) {
                        events[events_num].events = EPOLLIN;
                        events[events_num].data = ev.data;
                        events_num += 1;
                    } else if state.writable && (ev.events & EPOLLOUT != 0) {
                        events[events_num].events = EPOLLOUT;
                        events[events_num].data = ev.data;
                        events_num += 1;
                    }
                }
            }
        }
        Ok(events_num)
    }
}

impl FileLike for EpollInstance {
    fn read(&self, _buf: &mut [u8]) -> LinuxResult<usize> {
        Err(LinuxError::ENOSYS)
    }

    fn write(&self, _buf: &[u8]) -> LinuxResult<usize> {
        Err(LinuxError::ENOSYS)
    }

    fn stat(&self) -> LinuxResult<Kstat> {
        Ok(Kstat::default())
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn core::any::Any + Send + Sync> {
        self
    }

    fn poll(&self) -> LinuxResult<axio::PollState> {
        Ok(axio::PollState {
            readable: !self.events.lock().is_empty(),
            writable: false,
        })
    }

    fn set_nonblocking(&self, _nonblocking: bool) -> LinuxResult {
        Ok(())
    }
}

/// Implementation of epoll_create system call
pub fn sys_epoll_create(size: c_int) -> LinuxResult<isize> {
    debug!("sys_epoll_create <= size: {}", size);

    if size <= 0 {
        return Err(LinuxError::EINVAL);
    }

    let epoll_instance = Arc::new(EpollInstance::new(0));
    let fd = add_file_like(epoll_instance)?;
    Ok(fd as isize)
}

/// Implementation of epoll_create1 system call
pub fn sys_epoll_create1(flags: c_int) -> LinuxResult<isize> {
    debug!("sys_epoll_create1 <= flags: {}", flags);

    // For simplicity, ignore flags for now
    sys_epoll_create(1)
}

/// Implementation of epoll_ctl system call
pub fn sys_epoll_ctl(
    epfd: c_int,
    op: c_int,
    fd: c_int,
    event: UserPtr<EpollEvent>,
) -> LinuxResult<isize> {
    debug!("sys_epoll_ctl <= epfd: {}, op: {}, fd: {}", epfd, op, fd);

    // For EPOLL_CTL_DEL, event can be NULL
    let ev = if op as u32 == EPOLL_CTL_DEL {
        if event.is_null() {
            // Create a dummy event for DEL operation (event data is ignored)
            &EpollEvent { events: 0, data: 0 }
        } else {
            event.get_as_mut()?
        }
    } else {
        // For ADD and MOD operations, event must not be NULL
        event.get_as_mut()?
    };

    let ret = EpollInstance::from_fd(epfd)?.control(op as usize, fd as usize, ev)?;
    Ok(ret as isize)
}

/// Implementation of epoll_wait system call
pub fn sys_epoll_wait(
    epfd: c_int,
    events: UserPtr<EpollEvent>,
    maxevents: c_int,
    timeout: c_int,
) -> LinuxResult<isize> {
    debug!(
        "sys_epoll_wait <= epfd: {}, maxevents: {}, timeout: {}",
        epfd, maxevents, timeout
    );

    if maxevents <= 0 {
        return Err(LinuxError::EINVAL);
    }

    let deadline =
        (!timeout.is_negative()).then(|| wall_time() + Duration::from_millis(timeout as u64));
    let epoll_instance = EpollInstance::from_fd(epfd)?;

    loop {
        axnet::poll_interfaces();

        // Create a buffer to hold events
        let mut event_buffer = Vec::with_capacity(maxevents as usize);
        event_buffer.resize(maxevents as usize, EpollEvent { events: 0, data: 0 });

        let events_num = epoll_instance.poll_all(&mut event_buffer)?;
        if events_num > 0 {
            // Copy events back to user space
            let events_slice = events.get_as_mut_slice(events_num)?;
            events_slice[..events_num].copy_from_slice(&event_buffer[..events_num]);
            return Ok(events_num as isize);
        }

        if deadline.is_some_and(|ddl| wall_time() >= ddl) {
            return Ok(0);
        }

        axtask::sleep(Duration::from_millis(1));
    }
}

/// Implementation of epoll_pwait system call (simplified version without signal handling)
pub fn sys_epoll_pwait(
    epfd: c_int,
    events: UserPtr<EpollEvent>,
    maxevents: c_int,
    timeout: c_int,
    _sigmask: UserPtr<sigset_t>,
) -> LinuxResult<isize> {
    debug!(
        "sys_epoll_pwait <= epfd: {}, maxevents: {}, timeout: {}",
        epfd, maxevents, timeout
    );

    // For now, ignore signal mask and just call epoll_wait
    sys_epoll_wait(epfd, events, maxevents, timeout)
}
