//! poll and ppoll system calls

use core::{ffi::c_int, time::Duration};

use crate::file::get_file_like;
use crate::ptr::UserPtr;
use axerrno::LinuxResult;
use axhal::time::wall_time;
use linux_raw_sys::general::{POLLERR, POLLIN, POLLNVAL, POLLOUT, pollfd, sigset_t, timespec};

/// Implementation of poll system call
pub fn sys_poll(fds: UserPtr<pollfd>, nfds: usize, timeout_ms: c_int) -> LinuxResult<isize> {
    debug!(
        "sys_poll <= fds: {:?}, nfds: {}, timeout_ms: {}",
        fds.address(),
        nfds,
        timeout_ms
    );

    if nfds == 0 {
        if timeout_ms > 0 {
            axtask::sleep(Duration::from_millis(timeout_ms as u64));
        }
        return Ok(0);
    }

    let deadline =
        (!timeout_ms.is_negative()).then(|| wall_time() + Duration::from_millis(timeout_ms as u64));

    loop {
        axnet::poll_interfaces();

        let mut ready_count = 0;
        let pollfd_slice = fds.get_as_mut_slice(nfds)?;

        for pollfd_data in pollfd_slice.iter_mut().take(nfds) {
            pollfd_data.revents = 0;

            if pollfd_data.fd < 0 {
                continue;
            }

            match get_file_like(pollfd_data.fd) {
                Ok(file) => match file.poll() {
                    Ok(state) => {
                        if (pollfd_data.events & POLLIN as i16) != 0 && state.readable {
                            pollfd_data.revents |= POLLIN as i16;
                        }
                        if (pollfd_data.events & POLLOUT as i16) != 0 && state.writable {
                            pollfd_data.revents |= POLLOUT as i16;
                        }
                        if pollfd_data.revents != 0 {
                            ready_count += 1;
                        }
                    }
                    Err(_) => {
                        pollfd_data.revents = POLLERR as i16;
                        ready_count += 1;
                    }
                },
                Err(_) => {
                    pollfd_data.revents = POLLNVAL as i16;
                    ready_count += 1;
                }
            }
        }

        if ready_count > 0 {
            return Ok(ready_count);
        }

        if deadline.is_some_and(|ddl| wall_time() >= ddl) {
            return Ok(0);
        }

        axtask::sleep(Duration::from_millis(1));
    }
}

/// Implementation of ppoll system call (simplified version without signal handling)
pub fn sys_ppoll(
    fds: UserPtr<pollfd>,
    nfds: usize,
    timeout: UserPtr<timespec>,
    _sigmask: UserPtr<sigset_t>,
) -> LinuxResult<isize> {
    debug!("sys_ppoll <= fds: {:?}, nfds: {}", fds.address(), nfds);

    if nfds == 0 {
        if !timeout.is_null() {
            let ts = timeout.get_as_mut()?;
            let duration =
                Duration::from_secs(ts.tv_sec as u64) + Duration::from_nanos(ts.tv_nsec as u64);
            axtask::sleep(duration);
        }
        return Ok(0);
    }

    let deadline = if timeout.is_null() {
        None
    } else {
        let ts = timeout.get_as_mut()?;
        Some(
            wall_time()
                + Duration::from_secs(ts.tv_sec as u64)
                + Duration::from_nanos(ts.tv_nsec as u64),
        )
    };

    loop {
        axnet::poll_interfaces();

        let mut ready_count = 0;
        let pollfd_slice = fds.get_as_mut_slice(nfds)?;

        for pollfd_data in pollfd_slice.iter_mut().take(nfds) {
            pollfd_data.revents = 0;

            if pollfd_data.fd < 0 {
                continue;
            }

            match get_file_like(pollfd_data.fd) {
                Ok(file) => match file.poll() {
                    Ok(state) => {
                        if (pollfd_data.events & POLLIN as i16) != 0 && state.readable {
                            pollfd_data.revents |= POLLIN as i16;
                        }
                        if (pollfd_data.events & POLLOUT as i16) != 0 && state.writable {
                            pollfd_data.revents |= POLLOUT as i16;
                        }
                        if pollfd_data.revents != 0 {
                            ready_count += 1;
                        }
                    }
                    Err(_) => {
                        pollfd_data.revents = POLLERR as i16;
                        ready_count += 1;
                    }
                },
                Err(_) => {
                    pollfd_data.revents = POLLNVAL as i16;
                    ready_count += 1;
                }
            }
        }

        if ready_count > 0 {
            return Ok(ready_count);
        }

        if deadline.is_some_and(|ddl| wall_time() >= ddl) {
            return Ok(0);
        }

        axtask::sleep(Duration::from_millis(1));
    }
}
