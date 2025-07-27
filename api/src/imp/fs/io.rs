use alloc::{sync::Arc, vec};
use core::ffi::c_int;

use axerrno::{LinuxError, LinuxResult};
use axio::SeekFrom;
use linux_raw_sys::general::{__kernel_off_t, iovec};

use crate::{
    file::{FD_TABLE, File, FileLike, Pipe, get_file_like},
    ptr::{UserConstPtr, UserPtr},
};

const DEFAULT_BUFFER_SIZE: usize = 8192;

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

pub fn sys_copy_file_range(
    fd_in: c_int,
    off_in: UserPtr<__kernel_off_t>,
    fd_out: c_int,
    off_out: UserPtr<__kernel_off_t>,
    len: usize,
    _flags: u32,
) -> LinuxResult<isize> {
    debug!(
        "sys_copy_file_range <= fd_in: {}, fd_out: {}, len: {}",
        fd_in, fd_out, len
    );

    if fd_in == fd_out {
        return Err(LinuxError::EINVAL);
    }

    let file_in = File::from_fd(fd_in)?;
    let file_out = File::from_fd(fd_out)?;

    let mut buffer = vec![0u8; DEFAULT_BUFFER_SIZE.min(len)];
    let mut total_copied = 0;
    let mut remaining = len;

    while remaining > 0 {
        let chunk_size = DEFAULT_BUFFER_SIZE.min(remaining);

        let read_bytes = if off_in.is_null() {
            file_in.read(&mut buffer[..chunk_size])?
        } else {
            let offset_ref = off_in.get_as_mut()?;
            let current_offset = *offset_ref;
            let bytes = file_in.read_at(current_offset as u64, &mut buffer[..chunk_size])?;
            *offset_ref += bytes as __kernel_off_t;
            bytes
        };

        if read_bytes == 0 {
            break;
        }

        let written_bytes = if off_out.is_null() {
            file_out.write(&buffer[..read_bytes])?
        } else {
            let offset_ref = off_out.get_as_mut()?;
            let current_offset = *offset_ref;
            let bytes = file_out.write_at(current_offset as u64, &buffer[..read_bytes])?;
            *offset_ref += bytes as __kernel_off_t;
            bytes
        };

        total_copied += written_bytes;
        remaining -= written_bytes;

        if written_bytes < read_bytes {
            break;
        }
    }

    Ok(total_copied as isize)
}

pub fn sys_splice(
    fd_in: c_int,
    off_in: UserPtr<__kernel_off_t>,
    fd_out: c_int,
    off_out: UserPtr<__kernel_off_t>,
    len: usize,
    _flags: u32,
) -> LinuxResult<isize> {
    debug!(
        "sys_splice <= fd_in: {}, fd_out: {}, len: {}",
        fd_in, fd_out, len
    );

    if fd_in == fd_out {
        return Err(LinuxError::EINVAL);
    }

    let validate_offset = |offset_ptr: UserPtr<__kernel_off_t>| -> LinuxResult<()> {
        if !offset_ptr.is_null() {
            let offset = *offset_ptr.get_as_mut()?;
            if offset < 0 {
                return Err(LinuxError::EINVAL);
            }
        }
        Ok(())
    };

    validate_offset(off_in)?;
    validate_offset(off_out)?;

    let pipe_in = Pipe::from_fd(fd_in).ok();
    let pipe_out = Pipe::from_fd(fd_out).ok();

    match (pipe_in, pipe_out) {
        (Some(pipe), None) => {
            if !pipe.readable() {
                return Err(LinuxError::EPERM);
            }
            if off_out.is_null() {
                return Err(LinuxError::EINVAL);
            }

            let file_out = File::from_fd(fd_out)?;
            splice_pipe_to_file(pipe, file_out, off_out, len)
        }
        (None, Some(pipe)) => {
            if !pipe.writable() {
                return Err(LinuxError::EPERM);
            }
            if off_in.is_null() {
                return Err(LinuxError::EINVAL);
            }

            let file_in = File::from_fd(fd_in)?;
            splice_file_to_pipe(file_in, pipe, off_in, len)
        }
        _ => Err(LinuxError::EINVAL),
    }
}

fn splice_pipe_to_file(
    pipe: Arc<crate::file::Pipe>,
    file: Arc<File>,
    off_out: UserPtr<__kernel_off_t>,
    len: usize,
) -> LinuxResult<isize> {
    let mut buffer = vec![0u8; DEFAULT_BUFFER_SIZE.min(len)];
    let mut total_copied = 0;
    let mut remaining = len;

    while remaining > 0 {
        let available = pipe.available_data();
        if available == 0 {
            if pipe.closed() {
                break;
            }
            break;
        }

        let chunk_size = DEFAULT_BUFFER_SIZE.min(remaining).min(available);
        let bytes_read = pipe.read(&mut buffer[..chunk_size])?;

        if bytes_read == 0 {
            break;
        }

        let off_out_ref = off_out.get_as_mut()?;
        let current_off_out = *off_out_ref;
        let written_bytes = file.write_at(current_off_out as u64, &buffer[..bytes_read])?;
        *off_out_ref += written_bytes as __kernel_off_t;

        total_copied += written_bytes;
        remaining -= written_bytes;

        if written_bytes < bytes_read {
            break;
        }
    }

    Ok(total_copied as isize)
}

fn splice_file_to_pipe(
    file: Arc<File>,
    pipe: Arc<crate::file::Pipe>,
    off_in: UserPtr<__kernel_off_t>,
    len: usize,
) -> LinuxResult<isize> {
    let mut buffer = vec![0u8; DEFAULT_BUFFER_SIZE.min(len)];
    let mut total_copied = 0;
    let mut remaining = len;

    while remaining > 0 {
        let chunk_size = DEFAULT_BUFFER_SIZE.min(remaining);

        if !off_in.is_null() {
            let current_off_in = *off_in.get_as_mut()?;
            let file_stat = file.stat()?;
            if current_off_in >= file_stat.size() as __kernel_off_t {
                break;
            }
        }

        let off_in_ref = off_in.get_as_mut()?;
        let current_off_in = *off_in_ref;
        let read_bytes = file.read_at(current_off_in as u64, &mut buffer[..chunk_size])?;
        *off_in_ref += read_bytes as __kernel_off_t;
        if read_bytes == 0 {
            break;
        }

        let written_bytes = pipe.write(&buffer[..read_bytes])?;

        total_copied += written_bytes;
        remaining -= written_bytes;

        if written_bytes < read_bytes {
            break;
        }
    }

    Ok(total_copied as isize)
}
