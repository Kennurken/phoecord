//! The inline detour on the REST transport's `send` (task 2.2.2, part 1).
//!
//! `send(self, request)` takes the transport in `rcx` and the request in `rdx`; what it returns
//! (a boxed future) is not touched. So the detour is a tail call: a naked entry saves the argument
//! registers, calls [`on_send`] with them, restores them and jumps to the original code, which
//! returns straight to Discord's caller. The return registers never pass through Rust code, and
//! nothing depends on how `send` returns.
//!
//! [`on_send`] reads the request through [`ProcessMemory`] (`ReadProcessMemory` on this process:
//! a wrong offset is a refused read, not an access violation), hands it to the [`SendSink`] under
//! the panic guard and returns. Part 1 is read-only: a plugin verdict that would change the body
//! or answer locally is logged and the request goes on unchanged. Changing a body needs the
//! `Bytes` vtable of the client; a local answer needs the response layout (both open in the recon
//! notes). Until they land, the in-process transport cannot be the one that runs a launch (the
//! per-launch rule of 2.2.8 keeps the proxy in charge).

use super::abi::{self, AbiError, Body, Memory};
use super::guard::{Breaker, LogFn};
use super::http::HttpAction;
use crate::hook::{HookError, HookSet};
use std::cell::Cell;
use std::ffi::c_void;
use std::ops::Range;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use windows_sys::Win32::System::{
    Diagnostics::Debug::ReadProcessMemory, Threading::GetCurrentProcess,
};

/// Reads of this process's memory that fail instead of faulting.
pub struct ProcessMemory;

impl Memory for ProcessMemory {
    fn read(&self, addr: usize, buf: &mut [u8]) -> bool {
        if addr == 0 || buf.is_empty() {
            return buf.is_empty();
        }
        let mut done = 0usize;
        // SAFETY: `ReadProcessMemory` on our own process copies into `buf`, which is valid for
        // `buf.len()` bytes, and reports unreadable source pages as a failure instead of faulting.
        let ok = unsafe {
            ReadProcessMemory(
                GetCurrentProcess(),
                addr as *const c_void,
                buf.as_mut_ptr().cast(),
                buf.len(),
                &mut done,
            )
        };
        ok != 0 && done == buf.len()
    }
}

/// Where the detour hands what it read: the in-process transport (2.2.7).
pub trait SendSink: Send + Sync {
    /// Whether any plugin looks at this request; when not, the body is never read.
    fn wants(&self, method: &str, url: &str) -> bool;
    /// The plugin chain's verdict on the request.
    fn request(&self, method: &str, url: &str, body: Option<&[u8]>) -> HttpAction;
}

struct State {
    sink: Box<dyn SendSink>,
    image: Range<usize>,
    log: LogFn,
    breaker: Breaker,
}

static STATE: OnceLock<State> = OnceLock::new();
/// Entry of the relocated original `send`; the naked entry jumps through it.
static SEND_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// Kinds of problems already logged (one bit each), so a busy client cannot flood the log.
static LOGGED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// Set while this thread is inside [`on_send`]: a plugin request made from there must not
    /// re-enter the chain.
    static INSIDE: Cell<bool> = const { Cell::new(false) };
}

/// Prepares the detour on `send` at `target` (not active until the set is installed). `image` is
/// the client module's address range.
///
/// # Safety
/// `target` must be the entry of the client's `ReqwestTransport::send`, located by its signature,
/// and its first bytes must be whole instructions with no RIP-relative operand (checked for this
/// build in the recon notes).
pub unsafe fn prepare(
    set: &mut HookSet,
    target: usize,
    image: Range<usize>,
    sink: Box<dyn SendSink>,
    log: LogFn,
) -> Result<(), HookError> {
    let state = State {
        sink,
        image,
        log,
        breaker: Breaker::new("http_send"),
    };
    if STATE.set(state).is_err() {
        return Err(HookError::Detour(
            "the send detour is already prepared".into(),
        ));
    }
    // SAFETY: forwarded from the caller's contract; `send_entry` keeps the ABI of `send` because
    // it hands every register back unchanged before jumping to the original.
    let hook = unsafe { set.add(target as *const (), send_entry as *const ()) }?;
    // Stored before the set is installed, so the entry never jumps through a null pointer.
    SEND_ORIGINAL.store(hook.original() as usize, Ordering::Release);
    Ok(())
}

/// The detour itself. On entry `rsp` is 8 mod 16; four pushes and `0x28` keep `on_send`'s call
/// aligned and give it the 32-byte shadow space the Windows x64 ABI requires.
#[unsafe(naked)]
unsafe extern "C" fn send_entry() {
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
        "jmp qword ptr [rip + {original}]",
        hook = sym on_send,
        original = sym SEND_ORIGINAL,
    );
}

/// Runs on Discord's thread, before the original `send`. Never unwinds: the body is guarded.
extern "C" fn on_send(_transport: usize, request: usize) {
    let Some(state) = STATE.get() else { return };
    if INSIDE.with(|c| c.replace(true)) {
        return;
    }
    state
        .breaker
        .guard(&state.log, || (), || observe(state, request));
    INSIDE.with(|c| c.set(false));
}

const LOG_ABI: u32 = 1;
const LOG_OPAQUE: u32 = 2;
const LOG_VERDICT: u32 = 4;

fn once(state: &State, kind: u32, line: impl FnOnce() -> String) {
    if LOGGED.fetch_or(kind, Ordering::Relaxed) & kind == 0 {
        (state.log)(&line());
    }
}

fn observe(state: &State, request: usize) {
    let mem = ProcessMemory;
    let head = match abi::read_head(&mem, request) {
        Ok(h) => h,
        Err(e) => return abi_error(state, &e),
    };
    if !state.sink.wants(&head.method, &head.url) {
        return;
    }
    let body = match abi::read_body(&mem, request, state.image.clone()) {
        Body::None => None,
        Body::Bytes(b) => Some(b),
        Body::Opaque => {
            once(state, LOG_OPAQUE, || {
                format!(
                    "http_send: {} {}: body not readable in-process, plugins see none",
                    head.method,
                    super::http::route_of(&head.url)
                )
            });
            None
        }
    };
    match state.sink.request(&head.method, &head.url, body.as_deref()) {
        HttpAction::Pass => {}
        other => once(state, LOG_VERDICT, || {
            format!(
                "http_send: {} {}: {} is not applied in-process yet (2.2.2 part 2); request sent unchanged",
                head.method,
                super::http::route_of(&head.url),
                match other {
                    HttpAction::Reply { .. } => "a local reply",
                    _ => "a changed body",
                }
            )
        }),
    }
}

fn abi_error(state: &State, e: &AbiError) {
    once(state, LOG_ABI, || {
        format!("http_send: {e}; requests pass untouched (layout of this build differs?)")
    });
}
