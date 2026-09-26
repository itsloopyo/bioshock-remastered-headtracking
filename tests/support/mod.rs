//! The Rust side of tests/support/test_support.cpp, shared by the config tests.

#![allow(dead_code)]

use std::ffi::{c_char, c_int, c_void, CString};

use bioshock_headtrack::config::{Owner, RawOwner};

type EmitText = unsafe extern "C" fn(*mut c_void, *const c_char, usize);
type EmitPair = unsafe extern "C" fn(*mut c_void, *const c_char, usize, *const c_char, usize);
type EmitBinding = unsafe extern "C" fn(*mut c_void, u32, i32);

extern "C" {
    fn bsr_test_render_fresh(emit: EmitText, context: *mut c_void);
    fn bsr_test_render_loaded(owner: *const RawOwner, emit: EmitText, context: *mut c_void);
    fn bsr_test_hotkey_bindings(
        owner: *const RawOwner,
        which: c_int,
        emit: EmitBinding,
        context: *mut c_void,
    ) -> c_int;
    fn bsr_test_import_keys(emit: EmitPair, context: *mut c_void);
}

unsafe fn slice<'a>(data: *const c_char, len: usize) -> &'a [u8] {
    std::slice::from_raw_parts(data.cast(), len)
}

unsafe extern "C" fn take_bytes(context: *mut c_void, bytes: *const c_char, len: usize) {
    *context.cast::<Vec<u8>>() = slice(bytes, len).to_vec();
}

/// The file the owner creates where there is none: the table's fresh render.
pub fn render_fresh() -> Vec<u8> {
    let mut out = Vec::new();
    unsafe { bsr_test_render_fresh(take_bytes, (&mut out as *mut Vec<u8>).cast()) };
    out
}

/// Every setting the owner's last load gave, as the renderer writes them.
pub fn render_loaded(owner: &Owner) -> Vec<u8> {
    let mut out = Vec::new();
    unsafe { bsr_test_render_loaded(owner.raw(), take_bytes, (&mut out as *mut Vec<u8>).cast()) };
    out
}

/// The hotkey lists the mod registers.
#[derive(Clone, Copy)]
pub enum Hotkey {
    Toggle = 0,
    CycleMode = 1,
    YawMode = 2,
}

unsafe extern "C" fn take_binding(context: *mut c_void, modifiers: u32, vk: i32) {
    (*context.cast::<Vec<(u32, i32)>>()).push((modifiers, vk));
}

/// The (modifiers, virtual key) bindings the mod registers for one list of the last load.
pub fn hotkey_bindings(owner: &Owner, which: Hotkey) -> Vec<(u32, i32)> {
    let mut out = Vec::new();
    let status = unsafe {
        bsr_test_hotkey_bindings(
            owner.raw(),
            which as c_int,
            take_binding,
            (&mut out as *mut Vec<(u32, i32)>).cast(),
        )
    };
    assert_eq!(status, 0, "a loaded hotkey list does not parse");
    out
}

unsafe extern "C" fn take_key(
    context: *mut c_void,
    section: *const c_char,
    section_len: usize,
    key: *const c_char,
    key_len: usize,
) {
    (*context.cast::<Vec<(String, String)>>()).push((
        String::from_utf8(slice(section, section_len).to_vec()).unwrap(),
        String::from_utf8(slice(key, key_len).to_vec()).unwrap(),
    ));
}

/// The keys the C++ import names.
pub fn import_keys() -> Vec<(String, String)> {
    let mut out = Vec::new();
    unsafe { bsr_test_import_keys(take_key, (&mut out as *mut Vec<(String, String)>).cast()) };
    out
}

pub mod corpus {
    use super::*;

    #[repr(C)]
    struct LegacyKey {
        section: *const c_char,
        key: *const c_char,
    }

    #[repr(C)]
    struct MutationKey {
        section: *const c_char,
        key: *const c_char,
        alternate: *const c_char,
        out_of_range: *const *const c_char,
        out_of_range_len: usize,
        hotkey: c_int,
    }

    extern "C" {
        fn bsr_test_ini_mutations(
            base: *const c_char,
            base_len: usize,
            reads: *const LegacyKey,
            reads_len: usize,
            keys: *const MutationKey,
            keys_len: usize,
            emit: EmitPair,
            error: EmitText,
            context: *mut c_void,
        ) -> c_int;
    }

    /// One key the frozen reader reads, for core's corpus generator.
    pub struct Descriptor {
        pub section: &'static str,
        pub key: &'static str,
        pub alternate: &'static str,
        pub out_of_range: &'static [&'static str],
        pub hotkey: bool,
    }

    struct Sink {
        out: Vec<(String, Vec<u8>)>,
        error: Option<String>,
    }

    unsafe extern "C" fn emit(
        context: *mut c_void,
        name: *const c_char,
        name_len: usize,
        bytes: *const c_char,
        bytes_len: usize,
    ) {
        let sink = &mut *context.cast::<Sink>();
        sink.out.push((
            String::from_utf8(slice(name, name_len).to_vec()).unwrap(),
            slice(bytes, bytes_len).to_vec(),
        ));
    }

    unsafe extern "C" fn error(context: *mut c_void, text: *const c_char, len: usize) {
        let sink = &mut *context.cast::<Sink>();
        sink.error = Some(String::from_utf8_lossy(slice(text, len)).into_owned());
    }

    /// core's GenerateIniMutations over `base`, reading `reads` and describing each with
    /// `keys`. It refuses keys and descriptors that differ.
    pub fn generate(
        base: &[u8],
        reads: &[(&str, &str)],
        keys: &[Descriptor],
    ) -> Vec<(String, Vec<u8>)> {
        let c = |s: &str| CString::new(s).unwrap();
        let read_strings: Vec<(CString, CString)> =
            reads.iter().map(|(s, k)| (c(s), c(k))).collect();
        let read_keys: Vec<LegacyKey> = read_strings
            .iter()
            .map(|(s, k)| LegacyKey {
                section: s.as_ptr(),
                key: k.as_ptr(),
            })
            .collect();
        let key_strings: Vec<(CString, CString, CString, Vec<CString>)> = keys
            .iter()
            .map(|d| {
                (
                    c(d.section),
                    c(d.key),
                    c(d.alternate),
                    d.out_of_range.iter().map(|v| c(v)).collect(),
                )
            })
            .collect();
        let range_pointers: Vec<Vec<*const c_char>> = key_strings
            .iter()
            .map(|(_, _, _, r)| r.iter().map(|v| v.as_ptr()).collect())
            .collect();
        let mutation_keys: Vec<MutationKey> = keys
            .iter()
            .enumerate()
            .map(|(i, d)| MutationKey {
                section: key_strings[i].0.as_ptr(),
                key: key_strings[i].1.as_ptr(),
                alternate: key_strings[i].2.as_ptr(),
                out_of_range: range_pointers[i].as_ptr(),
                out_of_range_len: range_pointers[i].len(),
                hotkey: c_int::from(d.hotkey),
            })
            .collect();
        let mut sink = Sink {
            out: Vec::new(),
            error: None,
        };
        let status = unsafe {
            bsr_test_ini_mutations(
                base.as_ptr().cast(),
                base.len(),
                read_keys.as_ptr(),
                read_keys.len(),
                mutation_keys.as_ptr(),
                mutation_keys.len(),
                emit,
                error,
                (&mut sink as *mut Sink).cast(),
            )
        };
        assert_eq!(status, 0, "the corpus generator refused: {:?}", sink.error);
        sink.out
    }
}
