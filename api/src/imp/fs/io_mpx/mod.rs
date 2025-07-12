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

mod epoll;
mod poll;
mod select;

pub use self::epoll::*;
pub use self::poll::*;
pub use self::select::*;
