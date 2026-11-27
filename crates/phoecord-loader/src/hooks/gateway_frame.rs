//! Delivering server → client payloads plugins queue (`Host::emit_dispatch`) in an in-process
//! launch (tasks 2.2.3, 2.2.7): an inline detour on the client's per-frame handler
//! (`gateway_frame` locator).
//!
//! The inflate hook ([`super::gateway_recv`]) sees one frame per call and has no slot for an extra
//! one. Right after it, the inflate wrapper hands the frame's text to one handler (private recon
//! notes, live probes of 2026-11-27, plus a read of the wrapper's call). Its arguments, as the
//! wrapper builds them:
//!
//! | Register | What |
//! |:---|:---|
//! | `rcx` | the output (24 bytes, sret): `-2` in its first word on every frame seen live (HELLO, heartbeat ACKs, every dispatch) |
//! | `rdx` | the gateway session |
//! | `r8` | the frame: `{0, &Vec<u8>}`, the text borrowed, not moved (the wrapper reuses or frees the `Vec` afterwards) |
//! | `r9`, stack 5, stack 6 | a millisecond counter, a flag, a pointer: passed on unchanged |
//!
//! So a payload is delivered by calling the original handler once more with the same session and
//! extra arguments, a frame record of its own and the payload's JSON text in a `Vec` from the
//! client's allocator (whatever the `Vec` holds afterwards is freed the same way, as the wrapper
//! does). Payloads plugins queued before the current frame go in before the original call, those
//! they queued while handling it after, the order the proxy keeps. Nothing here runs the plugin
//! chain on a delivered payload, as in the proxy.
//!
//! A handler output other than `-2` for a delivered payload would be something the client should
//! act on (none was seen); it is logged once and left as is.

use super::guard::{Breaker, LogFn};
use crate::hook::{HookError, HookSet};
use phoecord_core::Val;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// The handler output's first word for "nothing to do".
pub const NOTHING: usize = usize::MAX - 1;
/// Room for the handler's output (24 bytes on this build), with slack.
const OUT_WORDS: usize = 8;

/// Payloads the transport queued outside a dispatch (a REST event, a timer).
pub trait FrameFeed: Send + Sync {
    fn take(&self) -> Vec<Val>;
}

struct State {
    feed: Box<dyn FrameFeed>,
    /// `__rust_alloc` and `__rust_dealloc`: the text `Vec` is the client's to free.
    allocator: (usize, usize),
    log: LogFn,
    breaker: Breaker,
}

static STATE: OnceLock<State> = OnceLock::new();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// Whether the detour is prepared: the inflate hook keeps queued payloads for it, else drops them.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static LOGGED: AtomicU32 = AtomicU32::new(0);
static DELIVERED: AtomicU32 = AtomicU32::new(0);

/// Whether queued payloads have a way to the client.
pub fn active() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// Prepares the detour on the handler at `target` (not active until the set is installed).
///
/// # Safety
/// `target` must be the entry of the client's per-frame gateway handler, located by its signature
/// (its first bytes are pushes, no RIP-relative operand); `allocator` the client's
/// `__rust_alloc` / `__rust_dealloc`.
pub unsafe fn prepare(
    set: &mut HookSet,
    target: usize,
    allocator: (usize, usize),
    feed: Box<dyn FrameFeed>,
    log: LogFn,
) -> Result<(), HookError> {
    let state = State {
        feed,
        allocator,
        log,
        breaker: Breaker::new("gateway_frame"),
    };
    if STATE.set(state).is_err() {
        return Err(HookError::Detour(
            "the gateway frame detour is already prepared".into(),
        ));
    }
    // SAFETY: forwarded; `frame_entry` passes all six arguments to the original and returns its
    // `rax`.
    let hook = unsafe { set.add(target as *const (), frame_entry as *const ()) }?;
    ORIGINAL.store(hook.original() as usize, Ordering::Release);
    ACTIVE.store(true, Ordering::Release);
    Ok(())
}

/// The detour. On entry `rsp` is 8 mod 16 and the stack arguments are at `+0x28` / `+0x30`; six
/// pushes and `0x58` realign it (the stack arguments are then at `+0xb0` / `+0xb8`). The
/// arguments live in callee-saved registers across the calls; `r9` and the two stack arguments
/// are also stored as three words, whose address [`before`] / [`after`] get.
#[unsafe(naked)]
unsafe extern "C" fn frame_entry() {
    std::arch::naked_asm!(
        "push rbx",
        "push rsi",
        "push rdi",
        "push r12",
        "push r13",
        "push r14",
        "sub rsp, 0x58",
        "mov rbx, rcx",
        "mov rsi, rdx",
        "mov rdi, r8",
        "mov r12, r9",
        "mov r13, [rsp + 0xb0]",
        "mov r14, [rsp + 0xb8]",
        "mov [rsp + 0x38], r12",
        "mov [rsp + 0x40], r13",
        "mov [rsp + 0x48], r14",
        "mov rcx, rsi",
        "lea rdx, [rsp + 0x38]",
        "call {before}",
        "mov rcx, rbx",
        "mov rdx, rsi",
        "mov r8, rdi",
        "mov r9, r12",
        "mov [rsp + 0x20], r13",
        "mov [rsp + 0x28], r14",
        "call qword ptr [rip + {original}]",
        "mov [rsp + 0x30], rax",
        "mov rcx, rsi",
        "lea rdx, [rsp + 0x38]",
        "call {after}",
        "mov rax, [rsp + 0x30]",
        "add rsp, 0x58",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rdi",
        "pop rsi",
        "pop rbx",
        "ret",
        before = sym before,
        after = sym after,
        original = sym ORIGINAL,
    );
}

/// Before the original: payloads queued before this frame.
extern "C" fn before(session: usize, extra: *const [usize; 3]) {
    let Some(state) = STATE.get() else { return };
    // SAFETY: `extra` points at the three words the entry stored on its own stack.
    let extra = unsafe { *extra };
    state.breaker.guard(
        &state.log,
        || (),
        || {
            let early = super::gateway_recv::stash_take_before();
            deliver_all(state, session, extra, early);
        },
    );
}

/// After the original: payloads queued while handling it, then any queued outside a frame.
extern "C" fn after(session: usize, extra: *const [usize; 3]) {
    let Some(state) = STATE.get() else { return };
    // SAFETY: as in `before`.
    let extra = unsafe { *extra };
    state.breaker.guard(
        &state.log,
        || (),
        || {
            let mut late = super::gateway_recv::stash_take_after();
            late.extend(state.feed.take());
            deliver_all(state, session, extra, late);
        },
    );
}

const LOG_FIRST: u32 = 1;
const LOG_SHAPE: u32 = 2;
const LOG_ALLOC: u32 = 4;
const LOG_OUTPUT: u32 = 8;

fn once(state: &State, kind: u32, line: impl FnOnce() -> String) {
    if LOGGED.fetch_or(kind, Ordering::Relaxed) & kind == 0 {
        (state.log)(&line());
    }
}

fn deliver_all(state: &State, session: usize, extra: [usize; 3], payloads: Vec<Val>) {
    for p in payloads {
        match frame_text(p) {
            Ok(text) => deliver(state, session, extra, &text),
            Err(e) => once(state, LOG_SHAPE, || {
                format!("gateway_frame: a payload plugins queued is not a dispatch ({e}); dropped")
            }),
        }
    }
}

/// The JSON text of one queued payload as a dispatch frame: `t` and `d` as the plugin gave them,
/// `op` 0, `s` as given or `null`.
pub fn frame_text(payload: Val) -> Result<Vec<u8>, String> {
    let Val::Map(_) = &payload else {
        return Err("not an object".into());
    };
    let t = payload
        .get("t")
        .and_then(Val::as_str)
        .ok_or("no event name")?
        .to_string();
    let d = payload.get("d").cloned().ok_or("no data")?;
    let s = payload.get("s").cloned().unwrap_or(Val::Null);
    let mut frame = Val::map();
    frame.set("t", Val::Str(t));
    frame.set("s", s);
    frame.set("op", Val::Int(0));
    frame.set("d", d);
    serde_json::to_vec(&frame.to_json()).map_err(|e| e.to_string())
}

/// Hands one frame's text to the client's handler.
fn deliver(state: &State, session: usize, extra: [usize; 3], text: &[u8]) {
    let (alloc, dealloc) = state.allocator;
    // SAFETY: `alloc` / `dealloc` are the client's `__rust_alloc(size, align)` and
    // `__rust_dealloc(ptr, size, align)`; `original` is the relocated handler with the argument
    // layout in the module docs. `vec` and `frame` live on this stack for the whole call, as the
    // wrapper's own do; the text is filled before the handler sees it, and whatever `vec` holds
    // afterwards is freed once, with the capacity it says.
    unsafe {
        let alloc: unsafe extern "C" fn(usize, usize) -> *mut u8 = std::mem::transmute(alloc);
        let dealloc: unsafe extern "C" fn(*mut u8, usize, usize) = std::mem::transmute(dealloc);
        let ptr = alloc(text.len(), 1);
        if ptr.is_null() {
            return once(state, LOG_ALLOC, || {
                "gateway_frame: the client's allocator refused a queued payload; dropped".into()
            });
        }
        std::ptr::copy_nonoverlapping(text.as_ptr(), ptr, text.len());
        let mut vec: [usize; 3] = [text.len(), ptr as usize, text.len()];
        let frame: [usize; 2] = [0, vec.as_mut_ptr() as usize];
        let mut out = [0usize; OUT_WORDS];
        let original: unsafe extern "C" fn(
            *mut usize,
            usize,
            *const usize,
            usize,
            usize,
            usize,
        ) -> usize = std::mem::transmute(ORIGINAL.load(Ordering::Acquire));
        original(
            out.as_mut_ptr(),
            session,
            frame.as_ptr(),
            extra[0],
            extra[1],
            extra[2],
        );
        let [cap, left, _] = vec;
        if cap > 0 {
            dealloc(left as *mut u8, cap, 1);
        }
        if out[0] != NOTHING {
            once(state, LOG_OUTPUT, || {
                format!(
                    "gateway_frame: the client's handler answered a delivered payload with {:#x}; left as is",
                    out[0]
                )
            });
        }
    }
    let n = DELIVERED.fetch_add(1, Ordering::Relaxed) + 1;
    once(state, LOG_FIRST, || {
        format!("gateway_frame: first queued payload delivered to the client ({n})")
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(json: &str) -> Val {
        Val::from_json(serde_json::from_str(json).unwrap())
    }

    #[test]
    fn a_payload_becomes_a_dispatch_frame() {
        let t = frame_text(val(r#"{"t":"MESSAGE_CREATE","d":{"id":"1"}}"#)).unwrap();
        let back: serde_json::Value = serde_json::from_slice(&t).unwrap();
        assert_eq!(
            back,
            serde_json::json!({"t":"MESSAGE_CREATE","s":null,"op":0,"d":{"id":"1"}})
        );
        let t = frame_text(val(r#"{"op":0,"t":"X","s":7,"d":null}"#)).unwrap();
        let back: serde_json::Value = serde_json::from_slice(&t).unwrap();
        assert_eq!(back["s"], 7);
    }

    #[test]
    fn a_payload_without_name_or_data_is_refused() {
        assert!(frame_text(val(r#"{"d":{}}"#)).is_err());
        assert!(frame_text(val(r#"{"t":"X"}"#)).is_err());
        assert!(frame_text(val(r#"[1]"#)).is_err());
    }
}
