//! User address space management.

use core::ffi::CStr;

use alloc::{borrow::ToOwned, string::String, vec, vec::Vec};
use axerrno::{AxError, AxResult};
use axhal::{
    mem::virt_to_phys,
    paging::{MappingFlags, PageSize},
};
use axmm::{AddrSpace, kernel_aspace};
use kernel_elf_parser::{AuxvEntry, ELFParser, app_stack_region};
use memory_addr::{MemoryAddr, PAGE_SIZE_4K, VirtAddr};
use xmas_elf::{ElfFile, program::SegmentData};

/// Creates a new empty user address space.
pub fn new_user_aspace_empty() -> AxResult<AddrSpace> {
    AddrSpace::new_empty(
        VirtAddr::from_usize(axconfig::plat::USER_SPACE_BASE),
        axconfig::plat::USER_SPACE_SIZE,
    )
}

/// If the target architecture requires it, the kernel portion of the address
/// space will be copied to the user address space.
pub fn copy_from_kernel(aspace: &mut AddrSpace) -> AxResult {
    if !cfg!(target_arch = "aarch64") && !cfg!(target_arch = "loongarch64") {
        // ARMv8 (aarch64) and LoongArch64 use separate page tables for user space
        // (aarch64: TTBR0_EL1, LoongArch64: PGDL), so there is no need to copy the
        // kernel portion to the user page table.
        aspace.copy_mappings_from(&kernel_aspace().lock())?;
    }
    Ok(())
}

/// Map the signal trampoline to the user address space.
pub fn map_trampoline(aspace: &mut AddrSpace) -> AxResult {
    let signal_trampoline_paddr = virt_to_phys(axsignal::arch::signal_trampoline_address().into());
    aspace.map_linear(
        axconfig::plat::SIGNAL_TRAMPOLINE.into(),
        signal_trampoline_paddr,
        PAGE_SIZE_4K,
        MappingFlags::READ | MappingFlags::EXECUTE | MappingFlags::USER,
        PageSize::Size4K,
    )?;
    Ok(())
}

/// Map the elf file to the user address space.
///
/// # Arguments
/// - `uspace`: The address space of the user app.
/// - `elf`: The elf file.
///
/// # Returns
/// - The entry point of the user app.
fn map_elf(uspace: &mut AddrSpace, elf: &ElfFile) -> AxResult<(VirtAddr, [AuxvEntry; 17])> {
    let uspace_base = uspace.base().as_usize();
    let elf_parser = ELFParser::new(
        elf,
        axconfig::plat::USER_INTERP_BASE,
        Some(uspace_base as isize),
        uspace_base,
    )
    .map_err(|_| AxError::InvalidData)?;

    for segement in elf_parser.ph_load() {
        debug!(
            "Mapping ELF segment: [{:#x?}, {:#x?}) flags: {:#x?}",
            segement.vaddr,
            segement.vaddr + segement.memsz as usize,
            segement.flags
        );
        let seg_pad = segement.vaddr.align_offset_4k();
        assert_eq!(seg_pad, segement.offset % PAGE_SIZE_4K);

        let seg_align_size =
            (segement.memsz as usize + seg_pad + PAGE_SIZE_4K - 1) & !(PAGE_SIZE_4K - 1);
        uspace.map_alloc(
            segement.vaddr.align_down_4k(),
            seg_align_size,
            segement.flags,
            true,
            PageSize::Size4K,
        )?;
        let seg_data = elf
            .input
            .get(segement.offset..segement.offset + segement.filesz as usize)
            .ok_or(AxError::InvalidData)?;
        uspace.write(segement.vaddr, PageSize::Size4K, seg_data)?;
        // TDOO: flush the I-cache
    }

    Ok((
        elf_parser.entry().into(),
        elf_parser.auxv_vector(PAGE_SIZE_4K),
    ))
}

/// Load the user app to the user address space.
///
/// # Arguments
/// - `uspace`: The address space of the user app.
/// - `args`: The arguments of the user app. The first argument is the path of the user app.
/// - `envs`: The environment variables of the user app.
///
/// # Returns
/// - The entry point of the user app.
/// - The stack pointer of the user app.
pub fn load_user_app(
    uspace: &mut AddrSpace,
    path: &str,
    args: &[String],
    envs: &[String],
) -> AxResult<(VirtAddr, VirtAddr)> {
    if args.is_empty() {
        return Err(AxError::InvalidInput);
    }
    let file_data = axfs::api::read(path)?;
    if file_data.starts_with(b"#!") {
        let head = &file_data[2..file_data.len().min(256)];
        let pos = head.iter().position(|c| *c == b'\n').unwrap_or(head.len());
        let line = core::str::from_utf8(&head[..pos]).map_err(|_| AxError::InvalidData)?;

        let new_args: Vec<String> = line
            .splitn(2, |c: char| c.is_ascii_whitespace())
            .map(|s| s.trim_ascii().to_owned())
            .chain(args.iter().cloned())
            .collect();
        return load_user_app(uspace, &new_args[0], &new_args, envs);
    }
    let elf = ElfFile::new(&file_data).map_err(|_| AxError::InvalidData)?;

    if let Some(interp) = elf
        .program_iter()
        .find(|ph| ph.get_type() == Ok(xmas_elf::program::Type::Interp))
    {
        let interp = match interp.get_data(&elf) {
            Ok(SegmentData::Undefined(data)) => data,
            _ => panic!("Invalid data in Interp Elf Program Header"),
        };

        let mut interp_path = axfs::api::canonicalize(
            CStr::from_bytes_with_nul(interp)
                .map_err(|_| AxError::InvalidData)?
                .to_str()
                .map_err(|_| AxError::InvalidData)?,
        )?;

        if interp_path == "/lib/ld-linux-riscv64-lp64.so.1"
            || interp_path == "/lib/ld-linux-riscv64-lp64d.so.1"
            || interp_path == "/lib64/ld-linux-loongarch-lp64d.so.1"
            || interp_path == "/lib64/ld-linux-x86-64.so.2"
            || interp_path == "/lib/ld-linux-aarch64.so.1"
        {
            // TODO: Use soft link
            interp_path = String::from("/musl/lib/libc.so");
        }

        // Set the first argument to the path of the user app.
        let mut new_args = vec![interp_path];
        new_args.extend_from_slice(args);
        return load_user_app(uspace, &new_args[0], &new_args, envs);
    }

    let (entry, mut auxv) = map_elf(uspace, &elf)?;
    // The user stack is divided into two parts:
    // `ustack_start` -> `ustack_pointer`: It is the stack space that users actually read and write.
    // `ustack_pointer` -> `ustack_end`: It is the space that contains the arguments, environment variables and auxv passed to the app.
    //  When the app starts running, the stack pointer points to `ustack_pointer`.
    let ustack_end = VirtAddr::from_usize(axconfig::plat::USER_STACK_TOP);
    let ustack_size = axconfig::plat::USER_STACK_SIZE;
    let ustack_start = ustack_end - ustack_size;
    debug!(
        "Mapping user stack: {:#x?} -> {:#x?}",
        ustack_start, ustack_end
    );

    let stack_data = app_stack_region(args, envs, &mut auxv, ustack_start, ustack_size);
    uspace.map_alloc(
        ustack_start,
        ustack_size,
        MappingFlags::READ | MappingFlags::WRITE | MappingFlags::USER,
        true,
        PageSize::Size4K,
    )?;

    let heap_start = VirtAddr::from_usize(axconfig::plat::USER_HEAP_BASE);
    let heap_size = axconfig::plat::USER_HEAP_SIZE;
    uspace.map_alloc(
        heap_start,
        heap_size,
        MappingFlags::READ | MappingFlags::WRITE | MappingFlags::USER,
        true,
        PageSize::Size4K,
    )?;

    let user_sp = ustack_end - stack_data.len();

    uspace.write(user_sp, PageSize::Size4K, stack_data.as_slice())?;

    Ok((entry, user_sp))
}

#[percpu::def_percpu]
static mut ACCESSING_USER_MEM: bool = false;

/// Enables scoped access into user memory, allowing page faults to occur inside
/// kernel.
pub fn access_user_memory<R>(f: impl FnOnce() -> R) -> R {
    ACCESSING_USER_MEM.with_current(|v| {
        *v = true;
        let result = f();
        *v = false;
        result
    })
}

/// Check if the current thread is accessing user memory.
pub fn is_accessing_user_memory() -> bool {
    ACCESSING_USER_MEM.read_current()
}
