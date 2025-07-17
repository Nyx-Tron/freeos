use core::ffi::c_int;

use axerrno::{LinuxError, LinuxResult};
use axio::SeekFrom;
use linux_raw_sys::general::{__kernel_off_t, iovec};

use crate::{
    file::{FD_TABLE, File, FileLike, get_file_like},
    ptr::{UserConstPtr, UserPtr},
};

/// Read data from the file indicated by `fd` at a specific offset.
///
/// This function reads up to `len` bytes from file descriptor `fd` at offset
/// `offset` into the buffer pointed to by `buf`. The file offset is not changed.
///
/// Return the number of bytes read if success.
pub fn sys_pread64(fd: c_int, buf: UserPtr<u8>, len: usize, offset: u64) -> LinuxResult<isize> {
    let buf = buf.get_as_mut_slice(len)?;
    debug!(
        "sys_pread64 <= fd: {}, buf: {:p}, len: {}, offset: {}",
        fd,
        buf.as_ptr(),
        buf.len(),
        offset
    );
    Ok(get_file_like(fd)?.read_at(offset, buf)? as _)
}

/// Write data to the file indicated by `fd` at a specific offset.
///
/// This function writes up to `len` bytes from the buffer pointed to by `buf`
/// to the file associated with the file descriptor `fd` at offset `offset`.
/// The file offset is not changed.
///
/// Return the number of bytes written if success.
pub fn sys_pwrite64(
    fd: c_int,
    buf: UserConstPtr<u8>,
    len: usize,
    offset: u64,
) -> LinuxResult<isize> {
    let buf = buf.get_as_slice(len)?;
    debug!(
        "sys_pwrite64 <= fd: {}, buf: {:p}, len: {}, offset: {}",
        fd,
        buf.as_ptr(),
        buf.len(),
        offset
    );
    Ok(get_file_like(fd)?.write_at(offset, buf)? as _)
}

/// Truncate a file to a specified length.
///
/// This function causes the regular file named by `fd` to be truncated to a size of
/// precisely `len` bytes. If the file previously was larger than this size, the extra
/// data is lost. If the file previously was shorter, it is extended, and the extended
/// part reads as null bytes.
///
/// Return 0 on success.
pub fn sys_ftruncate(fd: c_int, len: u64) -> LinuxResult<isize> {
    debug!("sys_ftruncate <= fd: {}, len: {}", fd, len);
    get_file_like(fd)?.truncate(len)?;
    Ok(0)
}

/// Synchronize a file's in-core state with storage device.
///
/// This function transfers ("flushes") all modified in-core data of the file
/// referred to by file descriptor `fd` to the disk device so that all changed
/// information can be retrieved even after the system crashed or was rebooted.
///
/// Return 0 on success.
pub fn sys_fsync(fd: c_int) -> LinuxResult<isize> {
    debug!("sys_fsync <= fd: {}", fd);
    get_file_like(fd)?.fsync()?;
    Ok(0)
}

/// Synchronize all file systems.
///
/// This function causes all pending modifications to filesystem metadata and
/// cached file data to be written to the underlying filesystems. It is equivalent
/// to calling fsync() on every open file descriptor.
///
/// Return 0 on success.
pub fn sys_sync() -> LinuxResult<isize> {
    debug!("sys_sync");
    FD_TABLE.sync_all()?;
    Ok(0)
}

/// Read data from the file indicated by `fd`.
///
/// Return the read size if success.
pub fn sys_read(fd: i32, buf: UserPtr<u8>, len: usize) -> LinuxResult<isize> {
    let buf = buf.get_as_mut_slice(len)?;
    debug!(
        "sys_read <= fd: {}, buf: {:p}, len: {}",
        fd,
        buf.as_ptr(),
        buf.len()
    );
    Ok(get_file_like(fd)?.read(buf)? as _)
}

/// Read data from the file using a vector of buffers.
///
/// This function performs the same task as multiple read() calls: it reads from
/// the file descriptor `fd` into multiple buffers as described by `iov`. The
/// `iocnt` argument specifies the number of elements in the `iov` array.
///
/// Return the total number of bytes read on success.
pub fn sys_readv(fd: i32, iov: UserPtr<iovec>, iocnt: usize) -> LinuxResult<isize> {
    if !(0..=1024).contains(&iocnt) {
        return Err(LinuxError::EINVAL);
    }

    let iovs = iov.get_as_mut_slice(iocnt)?;
    let mut ret = 0;
    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        let buf = UserPtr::<u8>::from(iov.iov_base as usize);
        let buf = buf.get_as_mut_slice(iov.iov_len as _)?;
        debug!(
            "sys_readv <= fd: {}, buf: {:p}, len: {}",
            fd,
            buf.as_ptr(),
            buf.len()
        );

        let read = get_file_like(fd)?.read(buf)?;
        ret += read as isize;

        if read < buf.len() {
            break;
        }
    }

    Ok(ret)
}

/// Write data to the file indicated by `fd`.
///
/// Return the written size if success.
pub fn sys_write(fd: i32, buf: UserConstPtr<u8>, len: usize) -> LinuxResult<isize> {
    let buf = buf.get_as_slice(len)?;
    debug!(
        "sys_write <= fd: {}, buf: {:p}, len: {}",
        fd,
        buf.as_ptr(),
        buf.len()
    );
    Ok(get_file_like(fd)?.write(buf)? as _)
}

/// Write data to the file using a vector of buffers.
///
/// This function performs the same task as multiple write() calls: it writes
/// data from multiple buffers as described by `iov` to the file descriptor `fd`.
/// The `iocnt` argument specifies the number of elements in the `iov` array.
///
/// Return the total number of bytes written on success.
pub fn sys_writev(fd: i32, iov: UserConstPtr<iovec>, iocnt: usize) -> LinuxResult<isize> {
    if !(0..=1024).contains(&iocnt) {
        return Err(LinuxError::EINVAL);
    }

    let iovs = iov.get_as_slice(iocnt)?;
    let mut ret = 0;
    for iov in iovs {
        if iov.iov_len == 0 {
            continue;
        }
        let buf = UserConstPtr::<u8>::from(iov.iov_base as usize);
        let buf = buf.get_as_slice(iov.iov_len as _)?;
        debug!(
            "sys_writev <= fd: {}, buf: {:p}, len: {}",
            fd,
            buf.as_ptr(),
            buf.len()
        );

        let written = get_file_like(fd)?.write(buf)?;
        ret += written as isize;

        if written < buf.len() {
            break;
        }
    }

    Ok(ret)
}

/// Reposition read/write file offset.
///
/// This function repositions the file offset of the open file description associated
/// with the file descriptor `fd` to the argument `offset` according to the directive
/// `whence`: SEEK_SET (0), SEEK_CUR (1), or SEEK_END (2).
///
/// Return the resulting offset location as measured in bytes from the beginning of the file.
pub fn sys_lseek(fd: c_int, offset: __kernel_off_t, whence: c_int) -> LinuxResult<isize> {
    debug!("sys_lseek <= {} {} {}", fd, offset, whence);
    let pos = match whence {
        0 => SeekFrom::Start(offset as _),
        1 => SeekFrom::Current(offset as _),
        2 => SeekFrom::End(offset as _),
        _ => return Err(LinuxError::EINVAL),
    };
    let off = File::from_fd(fd)?.inner().seek(pos)?;
    Ok(off as _)
}
