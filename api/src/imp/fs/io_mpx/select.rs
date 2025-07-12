//! select and pselect6 system calls

use core::{ffi::c_int, time::Duration};

use crate::file::get_file_like;
use crate::ptr::UserPtr;
use axerrno::{LinuxError, LinuxResult};
use axhal::time::wall_time;
use linux_raw_sys::general::{sigset_t, timespec, timeval};

const FD_SETSIZE: usize = 1024;
const BITS_PER_USIZE: usize = usize::BITS as usize;
const FD_SETSIZE_USIZES: usize = FD_SETSIZE.div_ceil(BITS_PER_USIZE);

/// fd_set structure for select system call  
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FdSet {
    pub fds_bits: [usize; FD_SETSIZE_USIZES],
}

struct FdSets {
    nfds: usize,
    bits: [usize; FD_SETSIZE_USIZES * 3],
}

impl FdSets {
    fn from(
        nfds: usize,
        read_fds: UserPtr<FdSet>,
        write_fds: UserPtr<FdSet>,
        except_fds: UserPtr<FdSet>,
    ) -> LinuxResult<Self> {
        let nfds = nfds.min(FD_SETSIZE);
        let nfds_usizes = nfds.div_ceil(BITS_PER_USIZE);
        let mut bits = [0usize; FD_SETSIZE_USIZES * 3];

        let copy_from_fd_set = |bits_slice: &mut [usize], fds: UserPtr<FdSet>| -> LinuxResult<()> {
            if fds.is_null() {
                bits_slice.fill(0);
            } else {
                let fd_set_ref = fds.get_as_mut()?;
                bits_slice[..nfds_usizes].copy_from_slice(&fd_set_ref.fds_bits[..nfds_usizes]);
            }
            Ok(())
        };

        copy_from_fd_set(&mut bits[0..FD_SETSIZE_USIZES], read_fds)?;
        copy_from_fd_set(
            &mut bits[FD_SETSIZE_USIZES..FD_SETSIZE_USIZES * 2],
            write_fds,
        )?;
        copy_from_fd_set(
            &mut bits[FD_SETSIZE_USIZES * 2..FD_SETSIZE_USIZES * 3],
            except_fds,
        )?;

        Ok(Self { nfds, bits })
    }

    fn poll_all(
        &self,
        res_read_fds: UserPtr<FdSet>,
        res_write_fds: UserPtr<FdSet>,
        res_except_fds: UserPtr<FdSet>,
    ) -> LinuxResult<usize> {
        let mut result_read = [0usize; FD_SETSIZE_USIZES];
        let mut result_write = [0usize; FD_SETSIZE_USIZES];
        let mut result_except = [0usize; FD_SETSIZE_USIZES];

        let mut read_bits_ptr = self.bits.as_ptr();
        let mut write_bits_ptr = unsafe { read_bits_ptr.add(FD_SETSIZE_USIZES) };
        let mut except_bits_ptr = unsafe { read_bits_ptr.add(FD_SETSIZE_USIZES * 2) };
        let mut i = 0;
        let mut res_num = 0;

        while i < self.nfds {
            let read_bits = unsafe { *read_bits_ptr };
            let write_bits = unsafe { *write_bits_ptr };
            let except_bits = unsafe { *except_bits_ptr };
            unsafe {
                read_bits_ptr = read_bits_ptr.add(1);
                write_bits_ptr = write_bits_ptr.add(1);
                except_bits_ptr = except_bits_ptr.add(1);
            }

            let all_bits = read_bits | write_bits | except_bits;
            if all_bits == 0 {
                i += BITS_PER_USIZE;
                continue;
            }

            let mut j = 0;
            while j < BITS_PER_USIZE && i + j < self.nfds {
                let bit = 1 << j;
                if all_bits & bit == 0 {
                    j += 1;
                    continue;
                }
                let fd = i + j;
                match get_file_like(fd as _).and_then(|f| f.poll()) {
                    Ok(state) => {
                        if state.readable && read_bits & bit != 0 {
                            let usize_idx = fd / BITS_PER_USIZE;
                            result_read[usize_idx] |= 1 << (fd % BITS_PER_USIZE);
                            res_num += 1;
                        }
                        if state.writable && write_bits & bit != 0 {
                            let usize_idx = fd / BITS_PER_USIZE;
                            result_write[usize_idx] |= 1 << (fd % BITS_PER_USIZE);
                            res_num += 1;
                        }
                    }
                    Err(_) => {
                        if except_bits & bit != 0 {
                            let usize_idx = fd / BITS_PER_USIZE;
                            result_except[usize_idx] |= 1 << (fd % BITS_PER_USIZE);
                            res_num += 1;
                        }
                    }
                }
                j += 1;
            }
            i += BITS_PER_USIZE;
        }

        // Write back results
        if !res_read_fds.is_null() {
            *res_read_fds.get_as_mut()? = FdSet {
                fds_bits: result_read,
            };
        }
        if !res_write_fds.is_null() {
            *res_write_fds.get_as_mut()? = FdSet {
                fds_bits: result_write,
            };
        }
        if !res_except_fds.is_null() {
            *res_except_fds.get_as_mut()? = FdSet {
                fds_bits: result_except,
            };
        }

        Ok(res_num)
    }
}

/// Implementation of select system call
pub fn sys_select(
    nfds: c_int,
    readfds: UserPtr<FdSet>,
    writefds: UserPtr<FdSet>,
    exceptfds: UserPtr<FdSet>,
    timeout: UserPtr<timeval>,
) -> LinuxResult<isize> {
    debug!("sys_select <= nfds: {}", nfds);

    if nfds < 0 {
        return Err(LinuxError::EINVAL);
    }

    let nfds = (nfds as usize).min(FD_SETSIZE);
    let deadline = if timeout.is_null() {
        None
    } else {
        let tv = timeout.get_as_mut()?;
        Some(
            wall_time()
                + Duration::from_secs(tv.tv_sec as u64)
                + Duration::from_micros(tv.tv_usec as u64),
        )
    };

    let fd_sets = FdSets::from(nfds, readfds, writefds, exceptfds)?;

    // Clear result fd_sets
    if !readfds.is_null() {
        *readfds.get_as_mut()? = FdSet {
            fds_bits: [0; FD_SETSIZE_USIZES],
        };
    }
    if !writefds.is_null() {
        *writefds.get_as_mut()? = FdSet {
            fds_bits: [0; FD_SETSIZE_USIZES],
        };
    }
    if !exceptfds.is_null() {
        *exceptfds.get_as_mut()? = FdSet {
            fds_bits: [0; FD_SETSIZE_USIZES],
        };
    }

    loop {
        axnet::poll_interfaces();

        let res = fd_sets.poll_all(readfds, writefds, exceptfds)?;
        if res > 0 {
            return Ok(res as isize);
        }

        if deadline.is_some_and(|ddl| wall_time() >= ddl) {
            return Ok(0);
        }

        axtask::sleep(Duration::from_millis(1));
    }
}

/// Implementation of pselect6 system call (simplified version without signal handling)
pub fn sys_pselect6(
    nfds: c_int,
    readfds: UserPtr<FdSet>,
    writefds: UserPtr<FdSet>,
    exceptfds: UserPtr<FdSet>,
    timeout: UserPtr<timespec>,
    _sigmask: UserPtr<sigset_t>,
) -> LinuxResult<isize> {
    debug!("sys_pselect6 <= nfds: {}", nfds);

    if nfds < 0 {
        return Err(LinuxError::EINVAL);
    }

    let nfds = (nfds as usize).min(FD_SETSIZE);
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

    let fd_sets = FdSets::from(nfds, readfds, writefds, exceptfds)?;

    // Clear result fd_sets
    if !readfds.is_null() {
        *readfds.get_as_mut()? = FdSet {
            fds_bits: [0; FD_SETSIZE_USIZES],
        };
    }
    if !writefds.is_null() {
        *writefds.get_as_mut()? = FdSet {
            fds_bits: [0; FD_SETSIZE_USIZES],
        };
    }
    if !exceptfds.is_null() {
        *exceptfds.get_as_mut()? = FdSet {
            fds_bits: [0; FD_SETSIZE_USIZES],
        };
    }

    loop {
        axnet::poll_interfaces();

        let res = fd_sets.poll_all(readfds, writefds, exceptfds)?;
        if res > 0 {
            return Ok(res as isize);
        }

        if deadline.is_some_and(|ddl| wall_time() >= ddl) {
            return Ok(0);
        }

        axtask::sleep(Duration::from_millis(1));
    }
}
