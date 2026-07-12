//! FFI misuse tests: invalid UTF-8, out-of-bounds ranges, NULL
//! arguments, generation-counter invalidation, calls after poisoning.

use kamitext::ffi::*;
use std::ptr;

fn new_engine(text: &str) -> *mut KamiEngine {
    let opts = KamiOptions {
        extensions: 0b111,
        reveal: 1, // Line
    };
    unsafe { kami_engine_new(text.as_ptr(), text.len(), opts) }
}

#[test]
fn abi_version_reported() {
    assert_eq!(kami_abi_version(), 1);
}

#[test]
fn create_query_free_roundtrip() {
    let e = new_engine("# **word**");
    assert!(!e.is_null());
    unsafe {
        assert_eq!(kami_len_bytes(e), 10);
        assert_eq!(kami_len_utf16(e), 10);

        let mut segs = KamiSegments {
            ptr: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(kami_segments_in(e, 0, 10, &mut segs), KAMI_OK);
        assert_eq!(segs.len, 4);
        let s0 = *segs.ptr;
        assert_eq!((s0.start, s0.end), (0, 2)); // "# " marker
        assert_eq!(s0.kinds, 1 << 21); // MARKER bit
        let s2 = *segs.ptr.add(2);
        assert_eq!((s2.start, s2.end), (4, 8)); // "word"
        assert_eq!(s2.kinds, (1 << 1) | (1 << 7)); // HEADING1|STRONG

        let mut txt = KamiStr {
            ptr: ptr::null(),
            len: 0,
        };
        assert_eq!(kami_text(e, &mut txt), KAMI_OK);
        let bytes = std::slice::from_raw_parts(txt.ptr, txt.len);
        assert_eq!(bytes, b"# **word**");

        kami_engine_free(e);
    }
}

#[test]
fn invalid_utf8_rejected() {
    let bad = [0xFFu8, 0xFE, 0x41];
    let opts = KamiOptions {
        extensions: 7,
        reveal: 1,
    };
    unsafe {
        // Constructor: NULL engine.
        let e = kami_engine_new(bad.as_ptr(), bad.len(), opts);
        assert!(e.is_null());

        // apply_edit: error code, engine untouched.
        let e = new_engine("abc");
        let mut patch = KamiPatch {
            ranges: ptr::null(),
            len: 0,
            generation: 0,
        };
        let rc = kami_apply_edit(e, 0, 0, bad.as_ptr(), bad.len(), &mut patch);
        assert_eq!(rc, KAMI_ERR_INVALID_UTF8);
        let msg = kami_last_error_message(e);
        assert!(msg.len > 0);
        assert_eq!(kami_len_bytes(e), 3);
        kami_engine_free(e);
    }
}

#[test]
fn invalid_options_rejected() {
    let opts = KamiOptions {
        extensions: 7,
        reveal: 99,
    };
    unsafe {
        let e = kami_engine_new(b"x".as_ptr(), 1, opts);
        assert!(e.is_null());
    }
}

#[test]
fn out_of_bounds_and_scalar_split_rejected() {
    let e = new_engine("a😀b");
    unsafe {
        let mut patch = KamiPatch {
            ranges: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(
            kami_apply_edit(e, 0, 99, b"x".as_ptr(), 1, &mut patch),
            KAMI_ERR_INVALID_RANGE
        );
        // Byte 2 splits the emoji scalar.
        assert_eq!(
            kami_apply_edit(e, 2, 3, b"x".as_ptr(), 1, &mut patch),
            KAMI_ERR_INVALID_RANGE
        );
        assert_eq!(kami_set_selection(e, 0, 99, &mut patch), KAMI_ERR_INVALID_RANGE);
        let msg = kami_last_error_message(e);
        let text = std::str::from_utf8(std::slice::from_raw_parts(msg.ptr, msg.len)).unwrap();
        assert!(text.contains("invalid range"), "got {text:?}");
        // Engine still healthy after rejected calls.
        assert_eq!(kami_len_bytes(e), 6);
        kami_engine_free(e);
    }
}

#[test]
fn error_message_clears_on_next_success() {
    let e = new_engine("abc");
    unsafe {
        let mut patch = KamiPatch {
            ranges: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(
            kami_apply_edit(e, 0, 99, b"x".as_ptr(), 1, &mut patch),
            KAMI_ERR_INVALID_RANGE
        );
        assert!(kami_last_error_message(e).len > 0);
        assert_eq!(kami_apply_edit(e, 0, 0, b"y".as_ptr(), 1, &mut patch), KAMI_OK);
        // A successful call leaves the message empty (header contract).
        assert_eq!(kami_last_error_message(e).len, 0);
        kami_engine_free(e);
    }
}

#[test]
fn null_arguments_safe() {
    unsafe {
        let mut patch = KamiPatch {
            ranges: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(
            kami_apply_edit(ptr::null_mut(), 0, 0, b"x".as_ptr(), 1, &mut patch),
            KAMI_ERR_NULL
        );
        assert_eq!(kami_len_bytes(ptr::null()), 0);
        assert_eq!(kami_generation(ptr::null()), 0);
        let msg = kami_last_error_message(ptr::null());
        assert_eq!(msg.len, 0);
        kami_engine_free(ptr::null_mut()); // no-op, must not crash

        let e = new_engine("abc");
        assert_eq!(kami_segments_in(e, 0, 3, ptr::null_mut()), KAMI_ERR_NULL);
        assert_eq!(kami_text(e, ptr::null_mut()), KAMI_ERR_NULL);
        // NULL out_patch is allowed (fire-and-forget edit).
        assert_eq!(
            kami_apply_edit(e, 0, 0, b"y".as_ptr(), 1, ptr::null_mut()),
            KAMI_OK
        );
        kami_engine_free(e);
    }
}

#[test]
fn generation_bumps_on_every_call_including_failures() {
    let e = new_engine("abc");
    unsafe {
        let g0 = kami_generation(e);
        let mut segs = KamiSegments {
            ptr: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(kami_segments_in(e, 0, 3, &mut segs), KAMI_OK);
        let g1 = kami_generation(e);
        assert!(g1 > g0);
        assert_eq!(segs.generation, g1);

        // A failing call also invalidates prior views.
        let mut patch = KamiPatch {
            ranges: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(
            kami_apply_edit(e, 0, 99, b"x".as_ptr(), 1, &mut patch),
            KAMI_ERR_INVALID_RANGE
        );
        assert!(kami_generation(e) > g1);
        kami_engine_free(e);
    }
}

#[test]
fn stale_views_are_scribbled_in_debug() {
    // Debug builds overwrite returned arenas at the next entry: a stale read
    // must observe poison values, not plausible data.
    if !cfg!(debug_assertions) {
        return;
    }
    let e = new_engine("# **word**");
    unsafe {
        let mut segs = KamiSegments {
            ptr: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(kami_segments_in(e, 0, 10, &mut segs), KAMI_OK);
        assert_eq!((*segs.ptr).start, 0);

        // Any subsequent call invalidates the view...
        let mut segs2 = KamiSegments {
            ptr: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(kami_segments_in(e, 0, 2, &mut segs2), KAMI_OK);

        // ...and the old pointer now reads scribbled memory (same allocation,
        // shorter fill; the tail slots must be poisoned).
        let stale = *segs.ptr.add(3);
        assert_eq!(stale.start, 0xDEAD_DEAD, "stale read must fail loudly");
        kami_engine_free(e);
    }
}

#[test]
fn poisoning_after_panic() {
    let e = new_engine("abc");
    unsafe {
        assert_eq!(kami_debug_force_panic(e), KAMI_ERR_INTERNAL);
        // All subsequent calls fail fast.
        let mut patch = KamiPatch {
            ranges: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(
            kami_apply_edit(e, 0, 0, b"x".as_ptr(), 1, &mut patch),
            KAMI_ERR_POISONED
        );
        let mut segs = KamiSegments {
            ptr: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(kami_segments_in(e, 0, 1, &mut segs), KAMI_ERR_POISONED);
        assert_eq!(kami_len_bytes(e), 0);
        // Error message survives and mentions the panic.
        let msg = kami_last_error_message(e);
        let text = std::str::from_utf8(std::slice::from_raw_parts(msg.ptr, msg.len)).unwrap();
        assert!(text.contains("panic"), "got {text:?}");
        // Free still works.
        kami_engine_free(e);
    }
}

#[test]
fn edit_plans_cross_the_boundary() {
    let e = new_engine("- [ ] todo");
    unsafe {
        let mut plan = KamiEditPlan {
            has_plan: 0,
            _pad: [0; 3],
            caret: 0,
            edits: ptr::null(),
            edits_len: 0,
            generation: 0,
        };
        assert_eq!(kami_toggle_task_plan(e, 7, &mut plan), KAMI_OK);
        assert_eq!(plan.has_plan, 1);
        assert_eq!(plan.edits_len, 1);
        let op = *plan.edits;
        assert_eq!((op.start, op.end), (2, 5));
        let repl = std::slice::from_raw_parts(op.text.ptr, op.text.len);
        assert_eq!(repl, b"[x]");
        assert_eq!(plan.caret, 7);

        // Newline continuation.
        assert_eq!(kami_newline_plan(e, 10, &mut plan), KAMI_OK);
        assert_eq!(plan.has_plan, 1);
        let op = *plan.edits;
        let repl = std::slice::from_raw_parts(op.text.ptr, op.text.len);
        assert_eq!(repl, b"\n- [ ] ");

        // No plan is KAMI_OK with has_plan == 0.
        let e2 = new_engine("plain");
        assert_eq!(kami_newline_plan(e2, 3, &mut plan), KAMI_OK);
        assert_eq!(plan.has_plan, 0);
        assert_eq!(kami_toggle_task_plan(e2, 3, &mut plan), KAMI_OK);
        assert_eq!(plan.has_plan, 0);
        // Misaligned offset is an error, not a missing plan.
        let e3 = new_engine("a😀b");
        assert_eq!(kami_newline_plan(e3, 2, &mut plan), KAMI_ERR_INVALID_RANGE);
        kami_engine_free(e3);
        kami_engine_free(e2);
        kami_engine_free(e);
    }
}

#[test]
fn patch_and_conversion_roundtrip() {
    let e = new_engine("héllo wörld");
    unsafe {
        let mut out = 0u32;
        assert_eq!(kami_byte_to_utf16(e, 3, &mut out), KAMI_OK);
        assert_eq!(out, 2); // h=1, é=2 bytes/1 unit
        assert_eq!(kami_utf16_to_byte(e, 2, &mut out), KAMI_OK);
        assert_eq!(out, 3);

        let mut patch = KamiPatch {
            ranges: ptr::null(),
            len: 0,
            generation: 0,
        };
        assert_eq!(kami_apply_edit(e, 0, 1, b"H".as_ptr(), 1, &mut patch), KAMI_OK);
        assert!(patch.len >= 1);
        let r0 = *patch.ranges;
        assert!(r0.start == 0 && r0.end >= 1);
        kami_engine_free(e);
    }
}
