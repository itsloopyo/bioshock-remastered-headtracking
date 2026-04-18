//! Memory scanning and PCV-discovery for the loaded game module.
//!
//! - `MemoryScanner` snapshots the host-process module range so we can
//!   safely walk it.
//! - `is_memory_valid` is the read-permission probe the engine hook
//!   uses before dereferencing candidate camera pointers.
//! - `find_player_calc_view_target` resolves the address of the UE2.5
//!   `eventPlayerCalcView` thunk via the FName-chain method documented
//!   in the project's CLAUDE.md.

use std::ffi::c_void;

use windows::core::PCSTR;
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::System::Memory::{
    VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_READONLY, PAGE_READWRITE, PAGE_WRITECOPY,
};
use windows::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
use windows::Win32::System::Threading::GetCurrentProcess;

/// Snapshot of the loaded game module's address range.
pub struct MemoryScanner {
    base_address: usize,
    module_size: usize,
}

impl MemoryScanner {
    /// Create a new scanner for the main game module (BioshockHD.exe).
    pub fn new() -> Option<Self> {
        unsafe {
            let module = GetModuleHandleA(PCSTR::null()).ok()?;
            let process = GetCurrentProcess();
            let mut mod_info = MODULEINFO::default();
            let result = GetModuleInformation(
                process,
                module,
                &mut mod_info,
                std::mem::size_of::<MODULEINFO>() as u32,
            );
            if result.is_err() {
                log::error!("Failed to get module information");
                return None;
            }
            Some(Self {
                base_address: mod_info.lpBaseOfDll as usize,
                module_size: mod_info.SizeOfImage as usize,
            })
        }
    }

    pub fn base(&self) -> usize {
        self.base_address
    }

    pub fn size(&self) -> usize {
        self.module_size
    }

    /// Find every occurrence of a wide (UTF-16LE-as-ASCII) string in
    /// the module.
    pub fn find_wide_string(&self, search_str: &str) -> Vec<usize> {
        let needle = to_wide_bytes(search_str);
        let mut results = Vec::new();
        if needle.is_empty() {
            return results;
        }
        unsafe {
            let memory =
                std::slice::from_raw_parts(self.base_address as *const u8, self.module_size);
            'outer: for i in 0..(self.module_size - needle.len()) {
                for j in 0..needle.len() {
                    if memory[i + j] != needle[j] {
                        continue 'outer;
                    }
                }
                results.push(self.base_address + i);
            }
        }
        results
    }

    /// Find all occurrences of a 32-bit immediate value in the module
    /// (CALL / JMP / MOV / LEA targets all show up). Used for xref
    /// resolution off a known address.
    pub fn find_references(&self, target_addr: usize) -> Vec<usize> {
        let mut results = Vec::new();
        let needle = (target_addr as u32).to_le_bytes();
        unsafe {
            let memory =
                std::slice::from_raw_parts(self.base_address as *const u8, self.module_size);
            for i in 0..(self.module_size - 4) {
                if memory[i] == needle[0]
                    && memory[i + 1] == needle[1]
                    && memory[i + 2] == needle[2]
                    && memory[i + 3] == needle[3]
                {
                    results.push(self.base_address + i);
                }
            }
        }
        results
    }
}

/// Check if a memory address is valid and readable.
pub fn is_memory_valid(address: usize, size: usize) -> bool {
    if address == 0 || address < 0x10000 {
        return false;
    }
    unsafe {
        let mut mbi = MEMORY_BASIC_INFORMATION::default();
        let result = VirtualQuery(
            Some(address as *const c_void),
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        );
        if result == 0 || mbi.State != MEM_COMMIT {
            return false;
        }
        let readable = mbi.Protect == PAGE_READWRITE
            || mbi.Protect == PAGE_READONLY
            || mbi.Protect == PAGE_EXECUTE_READ
            || mbi.Protect == PAGE_EXECUTE_READWRITE
            || mbi.Protect == PAGE_WRITECOPY
            || mbi.Protect == PAGE_EXECUTE_WRITECOPY;
        if !readable {
            return false;
        }
        let region_base = mbi.BaseAddress as usize;
        let region_end = region_base + mbi.RegionSize;
        address >= region_base && (address + size) <= region_end
    }
}

/// Convert ASCII string to wide (UTF-16LE) bytes.
fn to_wide_bytes(s: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(s.len() * 2);
    for c in s.chars() {
        result.push(c as u8);
        result.push(0);
    }
    result
}

/// Walk back from an instruction inside a function to its prologue
/// (`CC CC CC 55 8B EC` - MSVC int3 padding + `push ebp; mov ebp, esp`).
fn find_function_start(base: usize, size: usize, xref: usize) -> Option<usize> {
    const LOOKBACK_LIMIT: usize = 512;
    let off = xref.checked_sub(base)?;
    let lookback = off.min(LOOKBACK_LIMIT);
    let start = off - lookback;
    unsafe {
        let mem = std::slice::from_raw_parts(base as *const u8, size);
        let mut i = off;
        while i >= start + 3 {
            if mem[i - 3] == 0xCC
                && mem[i - 2] == 0xCC
                && mem[i - 1] == 0xCC
                && mem[i] == 0x55
                && mem[i + 1] == 0x8B
                && mem[i + 2] == 0xEC
            {
                return Some(base + i);
            }
            i -= 1;
        }
    }
    None
}

/// Given an FName-table init xref (`PUSH <wide-string-addr>`), walk
/// forward past the next `CALL` and find the next `MOV [imm32], ECX`.
/// That global is the storage for the FName.Index, and its other xrefs
/// are dispatch sites we can hook.
fn find_fname_index_global(base: usize, size: usize, xref: usize) -> Option<usize> {
    let off = xref.checked_sub(base)?;
    let end = (off + 96).min(size.saturating_sub(6));
    unsafe {
        let mem = std::slice::from_raw_parts(base as *const u8, size);
        // Find the next E8 rel32 (CALL) past the xref.
        let mut call_end: Option<usize> = None;
        let mut i = off;
        while i + 5 <= end {
            if mem[i] == 0xE8 {
                call_end = Some(i + 5);
                break;
            }
            i += 1;
        }
        let mut j = call_end?;
        // Find the next 89 0D imm32 (MOV [imm32], ECX).
        while j + 6 <= end {
            if mem[j] == 0x89 && mem[j + 1] == 0x0D {
                let imm = u32::from_le_bytes([mem[j + 2], mem[j + 3], mem[j + 4], mem[j + 5]]);
                return Some(imm as usize);
            }
            j += 1;
        }
    }
    None
}

/// Locate `APlayerController::eventPlayerCalcView` in the live module
/// via the FName-chain method:
///
///   1. Find the wide string `"PlayerCalcView"` in `.rdata`.
///   2. Find its xref - the `PUSH <addr>` inside the FName-table init
///      unrolled loop.
///   3. From that xref, walk forward to the `MOV [global], ECX` that
///      stores the resulting `FName.Index`. That global is
///      `NAME_PlayerCalcView.Index`.
///   4. The xrefs to that global (skipping the init-block writes near
///      the original string xref) are dispatch sites. Walk back from
///      each one to the containing function's MSVC prologue.
///
/// Returns the first plausible function start, or `None` if any stage
/// fails. Safer to not install a hook than to guess.
pub fn find_player_calc_view_target(scanner: &MemoryScanner) -> Option<usize> {
    let str_addr = scanner
        .find_wide_string("PlayerCalcView")
        .into_iter()
        .next()?;
    let init_xref = scanner.find_references(str_addr).into_iter().next()?;
    let name_global = find_fname_index_global(scanner.base(), scanner.size(), init_xref)?;
    for xref in scanner.find_references(name_global) {
        // Skip init-block writes - they sit within ~200 bytes of the
        // string xref because the whole name table lives in one
        // unrolled loop.
        if (xref as i64 - init_xref as i64).abs() < 200 {
            continue;
        }
        if let Some(fn_start) = find_function_start(scanner.base(), scanner.size(), xref) {
            return Some(fn_start);
        }
    }
    None
}
