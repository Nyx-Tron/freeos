//! Implements the node for /proc/self/exe.
use axfs_vfs::{VfsNodeAttr, VfsNodeOps, VfsNodeType, VfsResult};
use axtask::{TaskExtRef, current};

use crate::file::resolve_symlink_path;

/// SelfExe 结构体用于表示 /proc/self/exe 的符号链接节点。
/// 该节点用于获取当前进程的可执行文件路径。
pub struct SelfExe;

/// VfsNodeOps trait 的实现，提供符号链接相关操作。
impl VfsNodeOps for SelfExe {
    fn get_attr(&self) -> VfsResult<VfsNodeAttr> {
        Ok(VfsNodeAttr::new(
            axfs_vfs::VfsNodePerm::default_file(),
            VfsNodeType::SymLink,
            0,
            0,
        ))
    }

    fn readlink(&self, _path: &str, buf: &mut [u8]) -> VfsResult<usize> {
        let current = current();
        let target = current.task_ext().process_data().exe_path.read();
        let target = resolve_symlink_path(&target);
        let target_bytes = target.as_bytes();
        let copy_len = buf.len().min(target_bytes.len());
        buf[..copy_len].copy_from_slice(&target_bytes[..copy_len]);
        Ok(copy_len)
    }

    fn is_symlink(&self) -> bool {
        true
    }

    axfs_vfs::impl_vfs_non_dir_default! {}
}
