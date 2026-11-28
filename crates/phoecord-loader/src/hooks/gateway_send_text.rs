//! The outgoing gateway hook on `send_text` (task 2.2.4): an inline detour on the one function
//! every client → server text frame ends in (`gateway_send_text` locator; private recon notes,
//! 2.2.4 send path, 2026-11-27). It sees the encoder's frames and `IDENTIFY`, resume and the
//! heartbeat (`op 40` on Phoenix 0.1.0.2208), and it is the one place a frame can be dropped.
//!
//! `send_text(out = rcx, session = rdx, frame = r8)`: the naked [`send_text_entry`] calls
//! [`on_send_text`] first, with the argument registers kept. On a drop it returns `rax = out`
//! without calling the original; otherwise it jumps to the original with every register and the
//! stack as they were at entry. What happens to the frame is in [`super::send_text::handle`].
//!
//! When this locator is found, the encoder detour (`hooks::gateway_send`) is not installed: every
//! frame the encoder builds also reaches `send_text`, so both would run `Engine::on_send` on it.

use super::detour::ProcessMemory;
use super::gateway_send::FrameSink;
use super::guard::{Breaker, LogFn};
use super::send_text::{Heap, Outcome, handle};
use crate::hook::{HookError, HookSet};
use std::cell::Cell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

/// The client's allocator shims, `__rust_alloc(size, align)` / `__rust_dealloc(ptr, size,
/// align)`, and plain writes to its memory.
struct ClientHeap {
    alloc: usize,
    dealloc: usize,
}

impl Heap for ClientHeap {
    fn alloc(&self, len: usize) -> Option<usize> {
        // SAFETY: `alloc` is the client's `__rust_alloc` (scalar arguments: the same registers in
        // the Rust and C ABIs on Windows x64); alignment 1 as for a `String`.
        let p = unsafe {
            let f: unsafe extern "C" fn(usize, usize) -> *mut u8 = std::mem::transmute(self.alloc);
            f(len, 1)
        };
        (!p.is_null()).then_some(p as usize)
    }

    fn free(&self, ptr: usize, cap: usize) {
        // SAFETY: `ptr` is a `String` buffer of capacity `cap` the client allocated with the same
        // allocator, freed once (the caller no longer refers to it).
        unsafe {
            let f: unsafe extern "C" fn(*mut u8, usize, usize) = std::mem::transmute(self.dealloc);
            f(ptr as *mut u8, cap, 1);
        }
    }

    fn write(&self, addr: usize, bytes: &[u8]) {
        // SAFETY: `addr` is the `String` record or the result `send_text` was called with, or a
        // buffer `alloc` just returned: writable memory owned by this call.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len()) }
    }
}

struct State {
    sink: Box<dyn FrameSink>,
    log: LogFn,
    breaker: Breaker,
    heap: Option<ClientHeap>,
}

static STATE: OnceLock<State> = OnceLock::new();
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static LOGGED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

/// Prepares the detour on `send_text` at `target` (not active until the set is installed).
/// `allocator` is the client's `__rust_alloc` / `__rust_dealloc`, needed to change or drop a
/// frame.
///
/// # Safety
/// `target` must be the entry of the client's `send_text`, located by its signature, whose first
/// bytes are whole instructions with no RIP-relative operand; `allocator` must be the client's
/// allocator shims.
pub unsafe fn prepare(
    set: &mut HookSet,
    target: usize,
    allocator: Option<(usize, usize)>,
    sink: Box<dyn FrameSink>,
    log: LogFn,
) -> Result<(), HookError> {
    let state = State {
        sink,
        log,
        breaker: Breaker::new("gateway_send_text"),
        heap: allocator.map(|(alloc, dealloc)| ClientHeap { alloc, dealloc }),
    };
    if STATE.set(state).is_err() {
        return Err(HookError::Detour(
            "the gateway send_text detour is already prepared".into(),
        ));
    }
    // SAFETY: forwarded from the caller's contract; `send_text_entry` restores every argument
    // register and the stack before it jumps to the original.
    let hook = unsafe { set.add(target as *const (), send_text_entry as *const ()) }?;
    ORIGINAL.store(hook.original() as usize, Ordering::Release);
    Ok(())
}

/// The detour. At entry `rsp` is 8 mod 16; four pushes and `0x28` (shadow space included) align
/// the call. `rcx`, `rdx`, `r8` already are `on_send_text`'s arguments. The pops leave the flags
/// alone, so `al` is tested after them.
#[unsafe(naked)]
unsafe extern "C" fn send_text_entry() {
    std::arch::naked_asm!(
        "push rcx",
        "push rdx",
        "push r8",
        "push r9",
        "sub rsp, 0x28",
        "call {hook}",
        "add rsp, 0x28",
        "pop r9",
        "pop r8",
        "pop rdx",
        "pop rcx",
        "test al, al",
        "jnz 2f",
        "jmp qword ptr [rip + {original}]",
        "2:",
        "mov rax, rcx",
        "ret",
        hook = sym on_send_text,
        original = sym ORIGINAL,
    );
}

const LOG_FIRST: u32 = 1;
const LOG_READ: u32 = 2;
const LOG_NO_ALLOC: u32 = 4;
const LOG_ALLOC: u32 = 8;
const LOG_DROP: u32 = 16;
const LOG_HEARTBEAT: u32 = 32;

fn once(state: &State, kind: u32, line: impl FnOnce() -> String) {
    if LOGGED.fetch_or(kind, Ordering::Relaxed) & kind == 0 {
        (state.log)(&line());
    }
}

/// On entry of `send_text`, on the connection loop's thread. Returns 1 when the frame was
/// dropped (the original must not run).
extern "C" fn on_send_text(out: usize, _session: usize, frame: usize) -> u8 {
    let Some(state) = STATE.get() else { return 0 };
    if INSIDE.with(|c| c.replace(true)) {
        return 0;
    }
    let skip = state
        .breaker
        .guard(&state.log, || false, || observe(state, out, frame));
    INSIDE.with(|c| c.set(false));
    u8::from(skip)
}

fn observe(state: &State, out: usize, frame: usize) -> bool {
    once(state, LOG_FIRST, || {
        "gateway_send_text: first frame seen".into()
    });
    let heap = state.heap.as_ref().map(|h| h as &dyn Heap);
    let outcome = handle(&ProcessMemory, heap, out, frame, |text| {
        if text.starts_with(br#"{"op":40"#) {
            once(state, LOG_HEARTBEAT, || {
                "gateway_send_text: heartbeat (op 40) seen".into()
            });
        }
        state.sink.frame(text)
    });
    match &outcome {
        Outcome::Unreadable => once(state, LOG_READ, || {
            "gateway_send_text: the argument does not hold a frame string; frames pass untouched"
                .into()
        }),
        Outcome::NoAllocator { .. } => once(state, LOG_NO_ALLOC, || {
            "gateway_send_text: no allocator for this build; changed or dropped frames are sent unchanged".into()
        }),
        Outcome::AllocFailed => once(state, LOG_ALLOC, || {
            "gateway_send_text: the client's allocator refused a changed frame; sent unchanged"
                .into()
        }),
        Outcome::Dropped => once(state, LOG_DROP, || {
            "gateway_send_text: first frame dropped; the loop was told nothing is pending".into()
        }),
        Outcome::Passed | Outcome::Replaced => {}
    }
    outcome.skip()
}
