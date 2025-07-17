//! I/O multiplexing system calls
//!
//! * [`poll`](poll::sys_poll)
//! * [`ppoll`](poll::sys_ppoll)
//! * [`select`](select::sys_select)
//! * [`pselect6`](select::sys_pselect6)
//! * [`epoll_create1`](epoll::sys_epoll_create1)
//! * [`epoll_ctl`](epoll::sys_epoll_ctl)
//! * [`epoll_wait`](epoll::sys_epoll_wait)
//! * [`epoll_pwait`](epoll::sys_epoll_pwait)

use axerrno::LinuxResult;
use axhal::time::wall_time;
use core::time::Duration;

mod epoll;
mod poll;
mod select;

pub use self::epoll::*;
pub use self::poll::*;
pub use self::select::*;

/// Common polling loop that handles network polling, yielding, and timeout checking
/// Returns Ok(Some(result)) if polling function returns a result, Ok(None) if timeout occurred
pub(crate) fn poll_with_timeout<F, R>(
    deadline: Option<Duration>,
    mut poll_fn: F,
) -> LinuxResult<Option<R>>
where
    F: FnMut() -> LinuxResult<Option<R>>,
{
    loop {
        axnet::poll_interfaces();

        if let Some(result) = poll_fn()? {
            return Ok(Some(result));
        }

        axtask::yield_now();

        if deadline.is_some_and(|ddl| wall_time() >= ddl) {
            return Ok(None);
        }
    }
}

/// Handle empty nfds case with optional timeout sleep
pub(crate) fn handle_empty_nfds(timeout: Option<Duration>) -> LinuxResult<isize> {
    if let Some(duration) = timeout {
        axtask::sleep(duration);
    }
    Ok(0)
}
