//! C ABI.
//!
//! - All structs crossing the boundary are `#[repr(C)]` with fixed-width
//!   fields; results are `(ptr, len)` views into **engine-owned arenas**.
//! - **Lifetime rule (strict)**: every returned pointer is invalidated by any
//!   subsequent call on the same engine — mutating, querying, or failing.
//!   Copy out before the next call. Debug builds scribble the arenas at every
//!   entry (stale reads fail loudly) and stamp results with a generation
//!   counter (`kami_generation`) for adapter-side asserts.
//! - Every entry point is `catch_unwind`-wrapped: a panic returns
//!   `KAMI_ERR_INTERNAL` and poisons the engine; it never unwinds across the
//!   boundary.
//! - Strings passed in are `(ptr, len)` UTF-8, never NUL-terminated; invalid
//!   UTF-8 is rejected with `KAMI_ERR_INVALID_UTF8`, not lossy-converted.
//!
//! # Safety
//!
//! Every entry point shares one contract (documented here once rather than
//! per function): `engine` must be NULL or a live pointer from
//! [`kami_engine_new`] not yet freed; `(ptr, len)` pairs must describe `len`
//! readable bytes; out-pointers must be NULL or writable. NULL engine /
//! out-pointers are handled defensively (`KAMI_ERR_NULL`), dangling pointers
//! cannot be detected and are UB as in any C API.
#![allow(clippy::missing_safety_doc)]

use crate::engine::Engine;
use crate::types::{
    ByteRange, ElementKind, EngineOptions, Extensions, KamiError, RevealMode,
};
use std::panic::{catch_unwind, AssertUnwindSafe};

/// ABI version. Bumped on any breaking layout or semantic change.
pub const KAMI_ABI_VERSION: u32 = 4;

pub const KAMI_OK: i32 = 0;
/// Out-of-bounds, reversed or scalar-splitting range on a mutating call.
pub const KAMI_ERR_INVALID_RANGE: i32 = -1;
/// Input bytes are not valid UTF-8.
pub const KAMI_ERR_INVALID_UTF8: i32 = -2;
/// A required pointer argument was NULL.
pub const KAMI_ERR_NULL: i32 = -3;
/// The engine was poisoned by an earlier panic; re-create it.
pub const KAMI_ERR_POISONED: i32 = -4;
/// An internal panic was caught; the engine is now poisoned.
pub const KAMI_ERR_INTERNAL: i32 = -5;

/// Opaque engine handle (Rustonomicon opaque-struct pattern). Only ever used
/// behind a pointer; cbindgen emits a bare forward declaration.
pub struct KamiEngine {
    _data: [u8; 0],
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KamiOptions {
    /// Bitflags: 1 = tables, 2 = task lists, 4 = strikethrough, 8 = wikilinks.
    pub extensions: u32,
    /// 0 = none (reader), 1 = line (default), 2 = block, 3 = element.
    pub reveal: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KamiByteRange {
    pub start: u32,
    pub end: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KamiSegment {
    pub start: u32,
    pub end: u32,
    pub utf16_start: u32,
    pub utf16_end: u32,
    /// Kind bit set (stable ids).
    pub kinds: u64,
    /// 1 = concealed under the current reveal state.
    pub concealed: u8,
    pub _pad: [u8; 7],
}

pub const KAMI_ELEMENT_TASK: u32 = 0;
pub const KAMI_ELEMENT_LINK: u32 = 1;
pub const KAMI_ELEMENT_IMAGE: u32 = 2;
pub const KAMI_ELEMENT_FENCE: u32 = 3;
pub const KAMI_ELEMENT_WIKILINK: u32 = 4;
pub const KAMI_ELEMENT_HEADING: u32 = 5;

/// `KamiElement.flags` bit 0: this image came from Obsidian `![[…]]` embed
/// syntax rather than CommonMark `![](…)`. Hosts use it to decide percent
/// decoding (a wiki target is a literal vault path) and resizability (a `#`
/// fragment means something else inside `[[…]]`).
pub const KAMI_ELEMENT_FLAG_WIKI: u8 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KamiElement {
    pub id: u32,
    pub start: u32,
    pub end: u32,
    /// KAMI_ELEMENT_* tag. Unknown tags must be ignored.
    pub kind: u32,
    /// Task: 1 = checked. Heading: level 1–6. Other kinds: 0.
    pub checked: u8,
    /// KAMI_ELEMENT_FLAG_* bit set. 0 for every kind that defines no flag.
    pub flags: u8,
    pub _pad: [u8; 2],
    /// Link dest / image src / fence info / wikilink target / heading text
    /// byte range; 0-width when absent.
    pub aux_start: u32,
    pub aux_end: u32,
}

// The flag rides in reclaimed padding, so the layout MUST be unchanged for
// every field a host already reads. A silent break here is an ABI break.
const _: () = assert!(core::mem::size_of::<KamiElement>() == 28);
const _: () = assert!(core::mem::offset_of!(KamiElement, checked) == 16);
const _: () = assert!(core::mem::offset_of!(KamiElement, flags) == 17);
const _: () = assert!(core::mem::offset_of!(KamiElement, aux_start) == 20);
const _: () = assert!(core::mem::offset_of!(KamiElement, aux_end) == 24);

/// Borrowed UTF-8 view into engine memory. NOT NUL-terminated.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KamiStr {
    pub ptr: *const u8,
    pub len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KamiSegments {
    pub ptr: *const KamiSegment,
    pub len: usize,
    pub generation: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KamiElements {
    pub ptr: *const KamiElement,
    pub len: usize,
    pub generation: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KamiPatch {
    pub ranges: *const KamiByteRange,
    pub len: usize,
    pub generation: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KamiEditOp {
    pub start: u32,
    pub end: u32,
    /// Replacement text, engine-owned, NOT NUL-terminated.
    pub text: KamiStr,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KamiEditPlan {
    /// 1 when a plan exists; 0 = no plan (not an error).
    pub has_plan: u8,
    pub _pad: [u8; 3],
    /// Caret byte offset after applying the edits back-to-front.
    pub caret: u32,
    pub edits: *const KamiEditOp,
    pub edits_len: usize,
    pub generation: u64,
}

/// The real allocation behind a `*mut KamiEngine`.
struct Cell {
    engine: Engine,
    poisoned: bool,
    generation: u64,
    err: String,
    seg_buf: Vec<KamiSegment>,
    elem_buf: Vec<KamiElement>,
    range_buf: Vec<KamiByteRange>,
    op_buf: Vec<KamiEditOp>,
    /// Backing bytes for plan replacement strings (always valid UTF-8 while
    /// exposed; scribbled and cleared at every entry).
    str_buf: Vec<u8>,
}

impl Cell {
    /// Debug builds overwrite previously returned arena contents so stale
    /// pointer reads fail loudly.
    fn scribble(&mut self) {
        if cfg!(debug_assertions) {
            self.seg_buf.iter_mut().for_each(|s| {
                *s = KamiSegment {
                    start: 0xDEAD_DEAD,
                    end: 0xDEAD_DEAD,
                    utf16_start: 0xDEAD_DEAD,
                    utf16_end: 0xDEAD_DEAD,
                    kinds: u64::MAX,
                    concealed: 0xDD,
                    _pad: [0xDD; 7],
                }
            });
            self.elem_buf.iter_mut().for_each(|e| {
                *e = KamiElement {
                    id: 0xDEAD_DEAD,
                    start: 0xDEAD_DEAD,
                    end: 0xDEAD_DEAD,
                    kind: 0xDEAD_DEAD,
                    checked: 0xDD,
                    flags: 0xDD,
                    _pad: [0xDD; 2],
                    aux_start: 0xDEAD_DEAD,
                    aux_end: 0xDEAD_DEAD,
                }
            });
            self.range_buf.iter_mut().for_each(|r| {
                *r = KamiByteRange {
                    start: 0xDEAD_DEAD,
                    end: 0xDEAD_DEAD,
                }
            });
            self.op_buf.iter_mut().for_each(|o| {
                *o = KamiEditOp {
                    start: 0xDEAD_DEAD,
                    end: 0xDEAD_DEAD,
                    text: KamiStr {
                        ptr: core::ptr::null(),
                        len: 0,
                    },
                }
            });
            // Scribbling the string buffer invalidates prior text views.
            self.str_buf.iter_mut().for_each(|b| *b = 0xDD);
        }
        self.seg_buf.clear();
        self.elem_buf.clear();
        self.range_buf.clear();
        self.op_buf.clear();
        self.str_buf.clear();
    }
}

fn err_code(e: KamiError) -> i32 {
    match e {
        KamiError::InvalidRange => KAMI_ERR_INVALID_RANGE,
    }
}

/// Shared prologue: NULL check, generation bump, arena scribble, poison
/// check, catch_unwind with poisoning.
unsafe fn with_cell(engine: *mut KamiEngine, f: impl FnOnce(&mut Cell) -> i32) -> i32 {
    if engine.is_null() {
        return KAMI_ERR_NULL;
    }
    let cell = unsafe { &mut *(engine as *mut Cell) };
    cell.generation = cell.generation.wrapping_add(1);
    cell.scribble();
    if cell.poisoned {
        // Keep the panic message readable for poisoned-engine diagnostics.
        return KAMI_ERR_POISONED;
    }
    // A successful call must leave the error message empty (see
    // `kami_last_error_message`); error paths re-set it below.
    cell.err.clear();
    match catch_unwind(AssertUnwindSafe(|| f(&mut *cell))) {
        Ok(code) => code,
        Err(payload) => {
            cell.poisoned = true;
            cell.err.clear();
            let msg: &str = if let Some(s) = payload.downcast_ref::<&str>() {
                s
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s
            } else {
                "internal panic"
            };
            cell.err.push_str("panic: ");
            cell.err.push_str(msg);
            KAMI_ERR_INTERNAL
        }
    }
}

unsafe fn utf8_arg<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if ptr.is_null() {
        if len == 0 {
            return Some("");
        }
        return None;
    }
    let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
    core::str::from_utf8(bytes).ok()
}

fn options_from(o: KamiOptions) -> Option<EngineOptions> {
    let reveal = match o.reveal {
        0 => RevealMode::None,
        1 => RevealMode::Line,
        2 => RevealMode::Block,
        3 => RevealMode::Element,
        _ => return None,
    };
    Some(EngineOptions {
        extensions: Extensions::from_bits_truncate(o.extensions),
        reveal,
    })
}

fn fill_patch(cell: &mut Cell, dirty: &[ByteRange], out: &mut KamiPatch) {
    cell.range_buf.extend(dirty.iter().map(|r| KamiByteRange {
        start: r.start,
        end: r.end,
    }));
    out.ranges = cell.range_buf.as_ptr();
    out.len = cell.range_buf.len();
    out.generation = cell.generation;
}

// ------------------------------------------------------------ entry points

/// Adapters must check this before use.
#[unsafe(no_mangle)]
pub extern "C" fn kami_abi_version() -> u32 {
    KAMI_ABI_VERSION
}

/// Creates an engine from UTF-8 text. Returns NULL on invalid UTF-8, invalid
/// options, or internal panic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_engine_new(
    text: *const u8,
    text_len: usize,
    options: KamiOptions,
) -> *mut KamiEngine {
    let Some(text) = (unsafe { utf8_arg(text, text_len) }) else {
        return core::ptr::null_mut();
    };
    let Some(opts) = options_from(options) else {
        return core::ptr::null_mut();
    };
    let engine = match catch_unwind(|| Engine::new(text, opts)) {
        Ok(e) => e,
        Err(_) => return core::ptr::null_mut(),
    };
    let cell = Box::new(Cell {
        engine,
        poisoned: false,
        generation: 0,
        err: String::new(),
        seg_buf: Vec::new(),
        elem_buf: Vec::new(),
        range_buf: Vec::new(),
        op_buf: Vec::new(),
        str_buf: Vec::new(),
    });
    Box::into_raw(cell) as *mut KamiEngine
}

/// Frees the engine. NULL is a no-op. All outstanding views die with it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_engine_free(engine: *mut KamiEngine) {
    if !engine.is_null() {
        drop(unsafe { Box::from_raw(engine as *mut Cell) });
    }
}

/// Current view-invalidation counter. Every `with_cell` entry point (all
/// mutating and query calls) bumps it; the read-only observers
/// (`kami_generation`, `kami_len_bytes`, `kami_len_utf16`,
/// `kami_last_error_message`, `kami_abi_version`) do NOT bump it and do not
/// invalidate outstanding views — deliberately, so this counter can validate
/// a held view, and as a documented leniency over the strict
/// any-call-invalidates rule (conformant adapters are unaffected).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_generation(engine: *const KamiEngine) -> u64 {
    if engine.is_null() {
        return 0;
    }
    let cell = unsafe { &*(engine as *const Cell) };
    catch_unwind(AssertUnwindSafe(|| cell.generation)).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_apply_edit(
    engine: *mut KamiEngine,
    start: u32,
    end: u32,
    replacement: *const u8,
    replacement_len: usize,
    out_patch: *mut KamiPatch,
) -> i32 {
    unsafe {
        with_cell(engine, |cell| {
            let Some(repl) = utf8_arg(replacement, replacement_len) else {
                cell.err = "replacement is not valid UTF-8".into();
                return KAMI_ERR_INVALID_UTF8;
            };
            let result = cell.engine.apply_edit(ByteRange::new(start, end), repl);
            patch_response(cell, result, out_patch)
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_set_selection(
    engine: *mut KamiEngine,
    start: u32,
    end: u32,
    out_patch: *mut KamiPatch,
) -> i32 {
    unsafe {
        with_cell(engine, |cell| {
            let result = cell.engine.set_selection(ByteRange::new(start, end));
            patch_response(cell, result, out_patch)
        })
    }
}

/// Shared Ok/Err tail of the two patch-returning entry points: fill the
/// caller's out-patch view on success, record the error message on failure.
///
/// # Safety
/// `out_patch`, when non-null, must point to writable memory for one
/// `KamiPatch` (the entry points' own contract).
fn patch_response(
    cell: &mut Cell,
    result: Result<crate::types::Patch, KamiError>,
    out_patch: *mut KamiPatch,
) -> i32 {
    match result {
        Ok(patch) => {
            if !out_patch.is_null() {
                let mut view = KamiPatch {
                    ranges: core::ptr::null(),
                    len: 0,
                    generation: cell.generation,
                };
                fill_patch(cell, &patch.dirty, &mut view);
                // SAFETY: non-null per the check; validity is the entry
                // points' documented contract.
                unsafe { *out_patch = view };
            }
            KAMI_OK
        }
        Err(e) => {
            cell.err = e.to_string();
            err_code(e)
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_segments_in(
    engine: *mut KamiEngine,
    start: u32,
    end: u32,
    out: *mut KamiSegments,
) -> i32 {
    unsafe {
        with_cell(engine, |cell| {
            if out.is_null() {
                return KAMI_ERR_NULL;
            }
            let segs = cell.engine.segments_in(ByteRange::new(start, end));
            cell.seg_buf.extend(segs.iter().map(|s| KamiSegment {
                start: s.range.start,
                end: s.range.end,
                utf16_start: s.utf16.start,
                utf16_end: s.utf16.end,
                kinds: s.kinds.bits(),
                concealed: u8::from(s.concealed),
                _pad: [0; 7],
            }));
            *out = KamiSegments {
                ptr: cell.seg_buf.as_ptr(),
                len: cell.seg_buf.len(),
                generation: cell.generation,
            };
            KAMI_OK
        })
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_elements_in(
    engine: *mut KamiEngine,
    start: u32,
    end: u32,
    out: *mut KamiElements,
) -> i32 {
    unsafe {
        with_cell(engine, |cell| {
            if out.is_null() {
                return KAMI_ERR_NULL;
            }
            let els = cell.engine.elements_in(ByteRange::new(start, end));
            cell.elem_buf.extend(els.iter().map(|e| {
                let (kind, checked, flags, aux) = match e.kind {
                    ElementKind::Task { checked } => {
                        (KAMI_ELEMENT_TASK, u8::from(checked), 0, ByteRange::new(0, 0))
                    }
                    ElementKind::Link { dest } => (KAMI_ELEMENT_LINK, 0, 0, dest),
                    ElementKind::Image { src, wiki } => (
                        KAMI_ELEMENT_IMAGE,
                        0,
                        if wiki { KAMI_ELEMENT_FLAG_WIKI } else { 0 },
                        src,
                    ),
                    ElementKind::Fence { info } => (KAMI_ELEMENT_FENCE, 0, 0, info),
                    ElementKind::WikiLink { target } => (KAMI_ELEMENT_WIKILINK, 0, 0, target),
                    ElementKind::Heading { level, text } => (KAMI_ELEMENT_HEADING, level, 0, text),
                };
                KamiElement {
                    id: e.id,
                    start: e.range.start,
                    end: e.range.end,
                    kind,
                    checked,
                    flags,
                    _pad: [0; 2],
                    aux_start: aux.start,
                    aux_end: aux.end,
                }
            }));
            *out = KamiElements {
                ptr: cell.elem_buf.as_ptr(),
                len: cell.elem_buf.len(),
                generation: cell.generation,
            };
            KAMI_OK
        })
    }
}

/// Borrowed view of the whole document text (UTF-8, engine-owned).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_text(engine: *mut KamiEngine, out: *mut KamiStr) -> i32 {
    unsafe {
        with_cell(engine, |cell| {
            if out.is_null() {
                return KAMI_ERR_NULL;
            }
            let t = cell.engine.text();
            *out = KamiStr {
                ptr: t.as_ptr(),
                len: t.len(),
            };
            KAMI_OK
        })
    }
}

/// Read-only observer (see `kami_generation`): no generation bump, no view
/// invalidation. Returns 0 for a NULL or poisoned engine.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_len_bytes(engine: *const KamiEngine) -> u32 {
    if engine.is_null() {
        return 0;
    }
    let cell = unsafe { &*(engine as *const Cell) };
    if cell.poisoned {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| cell.engine.len_bytes())).unwrap_or(0)
}

/// Read-only observer (see `kami_generation`): no generation bump, no view
/// invalidation. Returns 0 for a NULL or poisoned engine.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_len_utf16(engine: *const KamiEngine) -> u32 {
    if engine.is_null() {
        return 0;
    }
    let cell = unsafe { &*(engine as *const Cell) };
    if cell.poisoned {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| cell.engine.len_utf16())).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_byte_to_utf16(
    engine: *mut KamiEngine,
    offset: u32,
    out: *mut u32,
) -> i32 {
    unsafe {
        with_cell(engine, |cell| {
            if out.is_null() {
                return KAMI_ERR_NULL;
            }
            *out = cell.engine.byte_to_utf16(offset);
            KAMI_OK
        })
    }
}

/// Rounds down to a scalar start — a query, never an error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_utf16_to_byte(
    engine: *mut KamiEngine,
    offset: u32,
    out: *mut u32,
) -> i32 {
    unsafe {
        with_cell(engine, |cell| {
            if out.is_null() {
                return KAMI_ERR_NULL;
            }
            *out = cell.engine.utf16_to_byte(offset);
            KAMI_OK
        })
    }
}

fn fill_plan(cell: &mut Cell, plan: Option<crate::types::EditPlan>, out: &mut KamiEditPlan) {
    let Some(plan) = plan else {
        out.has_plan = 0;
        out.caret = 0;
        out.edits = core::ptr::null();
        out.edits_len = 0;
        out.generation = cell.generation;
        return;
    };
    // Two passes: strings first (str_buf must stop reallocating before we
    // take pointers into it).
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(plan.edits.len());
    for (_, text) in &plan.edits {
        let s = cell.str_buf.len();
        cell.str_buf.extend_from_slice(text.as_bytes());
        spans.push((s, text.len()));
    }
    let base = cell.str_buf.as_ptr();
    for ((range, _), (off, len)) in plan.edits.iter().zip(spans) {
        cell.op_buf.push(KamiEditOp {
            start: range.start,
            end: range.end,
            text: KamiStr {
                ptr: unsafe { base.add(off) },
                len,
            },
        });
    }
    out.has_plan = 1;
    out.caret = plan.caret;
    out.edits = cell.op_buf.as_ptr();
    out.edits_len = cell.op_buf.len();
    out.generation = cell.generation;
}

/// List/task/quote continuation for Enter at `at`. `has_plan == 0`
/// with KAMI_OK means "insert a plain newline yourself".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_newline_plan(
    engine: *mut KamiEngine,
    at: u32,
    out: *mut KamiEditPlan,
) -> i32 {
    unsafe {
        with_cell(engine, |cell| {
            if out.is_null() {
                return KAMI_ERR_NULL;
            }
            match cell.engine.newline_plan(at) {
                Ok(plan) => {
                    let mut view = KamiEditPlan {
                        has_plan: 0,
                        _pad: [0; 3],
                        caret: 0,
                        edits: core::ptr::null(),
                        edits_len: 0,
                        generation: cell.generation,
                    };
                    fill_plan(cell, plan, &mut view);
                    *out = view;
                    KAMI_OK
                }
                Err(e) => {
                    cell.err = e.to_string();
                    err_code(e)
                }
            }
        })
    }
}

/// Flips the task checkbox whose item contains `at`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_toggle_task_plan(
    engine: *mut KamiEngine,
    at: u32,
    out: *mut KamiEditPlan,
) -> i32 {
    unsafe {
        with_cell(engine, |cell| {
            if out.is_null() {
                return KAMI_ERR_NULL;
            }
            match cell.engine.toggle_task_plan(at) {
                Ok(plan) => {
                    let mut view = KamiEditPlan {
                        has_plan: 0,
                        _pad: [0; 3],
                        caret: 0,
                        edits: core::ptr::null(),
                        edits_len: 0,
                        generation: cell.generation,
                    };
                    fill_plan(cell, plan, &mut view);
                    *out = view;
                    KAMI_OK
                }
                Err(e) => {
                    cell.err = e.to_string();
                    err_code(e)
                }
            }
        })
    }
}

/// Diagnostic message for the last error on this engine. Empty when the last
/// call succeeded. The view is engine-owned and dies at the next `with_cell`
/// call; it carries no generation token (same lifetime rule applies —
/// read-only observer, see `kami_generation`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_last_error_message(engine: *const KamiEngine) -> KamiStr {
    let empty = KamiStr {
        ptr: core::ptr::null(),
        len: 0,
    };
    if engine.is_null() {
        return empty;
    }
    let cell = unsafe { &*(engine as *const Cell) };
    catch_unwind(AssertUnwindSafe(|| KamiStr {
        ptr: cell.err.as_ptr(),
        len: cell.err.len(),
    }))
    .unwrap_or(empty)
}

/// Test hook: forces an internal panic so adapters can exercise poisoning.
/// Hidden from normal use; kept in release builds for adapter conformance
/// suites.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kami_debug_force_panic(engine: *mut KamiEngine) -> i32 {
    unsafe { with_cell(engine, |_| panic!("kami_debug_force_panic")) }
}
