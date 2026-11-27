//! Plugin entries in Phoenix's message context menu (task 2.3.5): the hook. Layout and the pure
//! steps are in [`crate::ui::message_menu`].
//!
//! Two patches:
//! - an inline detour (`retour`) on the helper that appends one entry record to the menu's `Vec`
//!   (`message_menu_push`). The naked [`push_entry`] calls the original, then [`on_pushed`] with the
//!   `Vec` and its own return address; when that is the call right after the native "Copy Message
//!   Link" entry (`message_menu_after_anchor`), PhoeCord's records are appended through the original
//!   helper, so the client's own code grows the `Vec`;
//! - the `call` of the build function in its live caller (`message_menu_build_call`) is pointed at
//!   a stub next to the client (a `call rel32` reaches ±2 GiB), which jumps to [`build_entry`]: it
//!   reads the channel and message ids from the build context and jumps on to the build function.
//!   The build function itself is not patched: another mod (Sunbird) already has a jump on its
//!   entry, and following it is not needed.
//!
//! A record's click handler is an `Rc<dyn Fn>`: PhoeCord's points at an `RcBox` it keeps for the
//! life of the process (counts so high they never reach zero, so the client never frees it) and a
//! vtable whose call slots are [`on_click`]. The click is handed to a [`MenuSink`] on a thread of
//! its own, never the client's UI thread.
//!
//! The menu is rebuilt on every frame while it is open, so the entries are read from a list a
//! background thread refreshes ([`refresh`]); a frame never waits for the proxy.

use super::abi::Memory;
use super::detour::ProcessMemory;
use super::guard::{Breaker, LogFn};
use crate::hook::{HookError, HookSet};
use crate::ui::message_menu::{self as layout, ITEM_SIZE, MenuTarget};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

/// One plugin entry to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub plugin: String,
    pub entry: String,
    pub label: String,
}

/// Where the hook gets its entries and sends its clicks: the engine of an in-process launch, or
/// the proxy over the loader IPC.
pub trait MenuSink: Send + Sync {
    /// The entries of the plugins that run now. May block (IPC); called on a background thread.
    fn items(&self) -> Vec<Item>;
    /// A click on `plugin`'s `entry` in the menu of `target`. Called on a thread of its own.
    fn click(&self, plugin: &str, entry: &str, target: MenuTarget);
}

/// An entry PhoeCord made a record for once; kept for the life of the process, because the
/// client may still hold its label and handler.
struct Slot {
    plugin: String,
    entry: String,
    label: &'static str,
    /// The `RcBox` of its handler: strong, weak, then the value (this slot's index).
    rc: usize,
    id: [u8; 16],
}

struct State {
    sink: Arc<dyn MenuSink>,
    log: LogFn,
    breaker: Breaker,
    after_anchor: usize,
    slots: Mutex<Vec<Arc<Slot>>>,
    /// The slots to show, in order; written by [`refresh`], read on every menu frame.
    shown: RwLock<Vec<Arc<Slot>>>,
    vtable: usize,
}

static STATE: OnceLock<State> = OnceLock::new();
/// The relocated original append helper.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);
/// The build function (the old target of the patched `call`).
static BUILDER: AtomicUsize = AtomicUsize::new(0);
/// The ids of the last menu built (0: unknown).
static CHANNEL: AtomicU64 = AtomicU64::new(0);
static MESSAGE: AtomicU64 = AtomicU64::new(0);
static LOGGED: AtomicU32 = AtomicU32::new(0);

const LOG_FIRST_BUILD: u32 = 1;
const LOG_NO_TARGET: u32 = 2;
const LOG_READ: u32 = 4;
const LOG_FIRST_APPEND: u32 = 8;
const LOG_FIRST_CLICK: u32 = 16;
const LOG_BUSY: u32 = 32;

/// How often the entry list is asked for again (plugins turned on or off, a language change).
const REFRESH: Duration = Duration::from_secs(3);
/// More than this many PhoeCord entries in one menu is a mistake; the rest are left out.
const MAX_SHOWN: usize = 32;

fn once(state: &State, kind: u32, line: impl FnOnce() -> String) {
    if LOGGED.fetch_or(kind, Ordering::Relaxed) & kind == 0 {
        (state.log)(&line());
    }
}

/// Where the hook goes, as absolute addresses in the running client.
pub struct Targets {
    /// Entry of the append helper.
    pub push: usize,
    /// Return address of the native anchor entry's append call.
    pub after_anchor: usize,
    /// The `call build` instruction (E8 rel32).
    pub build_call: usize,
}

/// Prepares the detour on the append helper and patches the build call at once; the detour is
/// active when the set is installed.
///
/// # Safety
/// The targets must be the unique matches of their locators in this client: `push` the entry of
/// the append helper (whole relocatable instructions first), `build_call` a `call rel32` whose
/// target is the menu build function, `after_anchor` the instruction after one of the helper's
/// calls.
pub unsafe fn prepare(
    set: &mut HookSet,
    t: Targets,
    sink: Arc<dyn MenuSink>,
    log: LogFn,
) -> Result<(), HookError> {
    let vtable: &'static [usize; 6] = Box::leak(Box::new([
        noop_drop as *const () as usize,
        16, // size of the value: the slot index and a spare word
        8,  // its alignment
        on_click as *const () as usize,
        on_click as *const () as usize,
        on_click as *const () as usize,
    ]));
    let state = State {
        sink,
        log,
        breaker: Breaker::new("message_menu"),
        after_anchor: t.after_anchor,
        slots: Mutex::new(Vec::new()),
        shown: RwLock::new(Vec::new()),
        vtable: vtable.as_ptr() as usize,
    };
    if STATE.set(state).is_err() {
        return Err(HookError::Detour(
            "the message menu hook is already prepared".into(),
        ));
    }
    // SAFETY: forwarded from the caller's contract; `push_entry` passes `rcx`/`rdx` to the original
    // and the helper returns nothing that matters.
    let hook = unsafe { set.add(t.push as *const (), push_entry as *const ()) }?;
    ORIGINAL.store(hook.original() as usize, Ordering::Release);
    // SAFETY: forwarded from the caller's contract for `build_call`.
    let builder = unsafe { redirect_call(t.build_call, build_entry as *const () as usize) }?;
    BUILDER.store(builder, Ordering::Release);
    let started = std::thread::Builder::new()
        .name("phoecord-menu".into())
        .spawn(|| {
            loop {
                refresh();
                std::thread::sleep(REFRESH);
            }
        });
    if let Err(e) = started {
        return Err(HookError::Detour(format!(
            "cannot start the menu thread: {e}"
        )));
    }
    Ok(())
}

/// Asks the sink for the entries and makes a slot for each new one (labels and handlers are never
/// freed: a frame of the client may still show an older list).
fn refresh() {
    let Some(state) = STATE.get() else { return };
    let items = state.sink.items();
    let mut slots = state.slots.lock().unwrap_or_else(|e| e.into_inner());
    let mut shown = Vec::new();
    for item in items.into_iter().take(MAX_SHOWN) {
        let existing = slots
            .iter()
            .find(|s| s.plugin == item.plugin && s.entry == item.entry && s.label == item.label);
        let slot = match existing {
            Some(s) => s.clone(),
            None => {
                let index = slots.len();
                let Some(id) = layout::inline_id(&layout::element_id(index)) else {
                    continue;
                };
                let rc: &'static mut [u64; 4] =
                    Box::leak(Box::new([1 << 40, 1 << 40, index as u64, 0]));
                let s = Arc::new(Slot {
                    plugin: item.plugin,
                    entry: item.entry,
                    label: Box::leak(item.label.into_boxed_str()),
                    rc: rc.as_ptr() as usize,
                    id,
                });
                slots.push(s.clone());
                s
            }
        };
        shown.push(slot);
    }
    *state.shown.write().unwrap_or_else(|e| e.into_inner()) = shown;
}

/// The detour of the append helper `push(vec, item)`. `rsi`/`rdi` (callee-saved) keep the `Vec`
/// and the return address across the original. Alignment: two pushes and `0x38`.
#[unsafe(naked)]
unsafe extern "C" fn push_entry() {
    std::arch::naked_asm!(
        "push rsi",
        "push rdi",
        "sub rsp, 0x38",
        "mov rsi, rcx",
        "mov rdi, [rsp + 0x48]",
        "call qword ptr [rip + {original}]",
        "mov rcx, rsi",
        "mov rdx, rdi",
        "call {hook}",
        "add rsp, 0x38",
        "pop rdi",
        "pop rsi",
        "ret",
        hook = sym on_pushed,
        original = sym ORIGINAL,
    );
}

/// Where the patched `call build` lands (through the stub): reads the ids from the context in
/// `rdx`, then jumps to the build function with every argument register as it came.
#[unsafe(naked)]
unsafe extern "C" fn build_entry() {
    std::arch::naked_asm!(
        "push rcx",
        "push rdx",
        "push r8",
        "push r9",
        "sub rsp, 0x28",
        "mov rcx, rdx",
        "call {hook}",
        "add rsp, 0x28",
        "pop r9",
        "pop r8",
        "pop rdx",
        "pop rcx",
        "jmp qword ptr [rip + {builder}]",
        hook = sym on_build,
        builder = sym BUILDER,
    );
}

extern "C" fn on_build(ctx: usize) {
    let Some(state) = STATE.get() else { return };
    state.breaker.guard(&state.log, || (), || {
        once(state, LOG_FIRST_BUILD, || "message_menu: first menu built".into());
        let target = layout::read_target(ctx, |a| ProcessMemory.word(a));
        let (c, m) = target.map_or((0, 0), |t| (t.channel_id, t.message_id));
        if target.is_none() {
            once(state, LOG_NO_TARGET, || {
                "message_menu: the build context holds no channel/message ids; clicks are not sent"
                    .into()
            });
        }
        CHANNEL.store(c, Ordering::Relaxed);
        MESSAGE.store(m, Ordering::Relaxed);
    });
}

/// After the original helper appended a record to `vec`, called from `ret`.
extern "C" fn on_pushed(vec: usize, ret: usize) {
    let Some(state) = STATE.get() else { return };
    if ret != state.after_anchor {
        return;
    }
    state
        .breaker
        .guard(&state.log, || (), || append(state, vec));
}

fn append(state: &State, vec: usize) {
    // A frame never waits for `refresh`: with the list being replaced, this frame shows none.
    let Ok(shown) = state.shown.try_read() else {
        return once(state, LOG_BUSY, || {
            "message_menu: the entry list was being refreshed; one frame without PhoeCord entries"
                .into()
        });
    };
    if shown.is_empty() {
        return;
    }
    let mem = ProcessMemory;
    let (Some(ptr), Some(len)) = (mem.word(vec + 8), mem.word(vec + 0x10)) else {
        return once(state, LOG_READ, || {
            "message_menu: the entry Vec is not readable".into()
        });
    };
    let mut template = [0u8; ITEM_SIZE];
    if len == 0 || !mem.read(ptr + (len - 1) * ITEM_SIZE, &mut template) {
        return once(state, LOG_READ, || {
            "message_menu: the native entry record is not readable".into()
        });
    }
    let original = ORIGINAL.load(Ordering::Acquire);
    if original == 0 {
        return;
    }
    // SAFETY: `original` is the relocated append helper `push(vec: &mut Vec<Item>, item: *const
    // Item)`, which copies `ITEM_SIZE` bytes from `item` (growing the Vec through the client's
    // allocator first when it is full). `vec` is the Vec it was just called with, on this thread.
    let push: unsafe extern "C" fn(usize, *const u8) = unsafe { std::mem::transmute(original) };
    for slot in shown.iter() {
        let record = layout::build_record(
            &template,
            slot.rc,
            state.vtable,
            slot.label.as_ptr() as usize,
            slot.label.len(),
            &slot.id,
        );
        // SAFETY: see above; `record` is a complete record on our stack for the call.
        unsafe { push(vec, record.as_ptr()) };
    }
    once(state, LOG_FIRST_APPEND, || {
        format!(
            "message_menu: {} PhoeCord entr(y/ies) shown in a menu",
            shown.len()
        )
    });
}

/// Drop of the handler value: nothing to free (it lives in PhoeCord's `RcBox`).
extern "C" fn noop_drop(_value: usize) {}

/// The handler's call slots: `Fn(&ClickEvent, &mut Window, &mut App)` with `self` (the value in
/// PhoeCord's `RcBox`: the slot index) first.
extern "C" fn on_click(value: *const u64, _event: usize, _window: usize, _app: usize) {
    let Some(state) = STATE.get() else { return };
    state.breaker.guard(
        &state.log,
        || (),
        || {
            // SAFETY: the client calls this with `self` = the value of an `RcBox` PhoeCord made in
            // `refresh` (leaked, so always valid); its first word is the slot index.
            let index = unsafe { *value } as usize;
            let slot = {
                let slots = state.slots.lock().unwrap_or_else(|e| e.into_inner());
                slots.get(index).cloned()
            };
            let Some(slot) = slot else { return };
            once(state, LOG_FIRST_CLICK, || {
                format!("message_menu: first click ({}/{})", slot.plugin, slot.entry)
            });
            let (c, m) = (
                CHANNEL.load(Ordering::Relaxed),
                MESSAGE.load(Ordering::Relaxed),
            );
            if c == 0 || m == 0 {
                return once(state, LOG_NO_TARGET, || {
                    "message_menu: a click without the menu's channel/message ids; not sent".into()
                });
            }
            let sink = state.sink.clone();
            let target = MenuTarget {
                channel_id: c,
                message_id: m,
            };
            let _ = std::thread::Builder::new()
                .name("phoecord-menu-click".into())
                .spawn(move || sink.click(&slot.plugin, &slot.entry, target));
        },
    );
}

/// Points the `call rel32` at `site` to `to` through a stub allocated within its reach. Returns the
/// call's old target.
///
/// # Safety
/// `site` must be a `call rel32` instruction in the client's code.
unsafe fn redirect_call(site: usize, to: usize) -> Result<usize, HookError> {
    use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
    use windows_sys::Win32::System::Memory::{PAGE_EXECUTE_READWRITE, VirtualProtect};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut code = [0u8; 5];
    if !ProcessMemory.read(site, &mut code) || code[0] != 0xE8 {
        return Err(HookError::Detour(format!(
            "message_menu_build_call is not a call rel32 ({code:02x?})"
        )));
    }
    let rel = i32::from_le_bytes([code[1], code[2], code[3], code[4]]);
    let old = (site as i64 + 5 + rel as i64) as usize;
    let stub = alloc_near(site)
        .ok_or_else(|| HookError::Detour("no memory within reach of the build call".into()))?;
    // `jmp qword ptr [rip + 0]` followed by the absolute address.
    let mut bytes = [0u8; 14];
    bytes[..6].copy_from_slice(&[0xFF, 0x25, 0, 0, 0, 0]);
    bytes[6..].copy_from_slice(&(to as u64).to_le_bytes());
    // SAFETY: `stub` is a fresh read-write-execute page of ours, at least 14 bytes.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), stub as *mut u8, bytes.len()) };
    let new_rel = (stub as i64) - (site as i64 + 5);
    let new_rel = i32::try_from(new_rel)
        .map_err(|_| HookError::Detour("the stub is out of a call's reach".into()))?;
    let patched = crate::hook::while_suspended(&[site], || {
        let mut old_protect = 0;
        // SAFETY: four bytes of the client's code at `site + 1`, made writable for the write and
        // restored; every other thread is suspended and none is inside the instruction.
        unsafe {
            if VirtualProtect(
                (site + 1) as *const _,
                4,
                PAGE_EXECUTE_READWRITE,
                &mut old_protect,
            ) == 0
            {
                return false;
            }
            std::ptr::write_unaligned((site + 1) as *mut i32, new_rel);
            let mut tmp = 0;
            VirtualProtect((site + 1) as *const _, 4, old_protect, &mut tmp);
            FlushInstructionCache(GetCurrentProcess(), site as *const _, 5);
        }
        true
    })?;
    if !patched {
        return Err(HookError::Detour(
            "the build call could not be made writable".into(),
        ));
    }
    Ok(old)
}

/// A read-write-execute page within ±2 GiB of `near` (64 KiB steps, nearest first).
fn alloc_near(near: usize) -> Option<usize> {
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READWRITE, VirtualAlloc,
    };
    const STEP: usize = 0x1_0000;
    const RANGE: usize = 0x7000_0000;
    let base = near & !(STEP - 1);
    for k in 1..RANGE / STEP {
        for addr in [base.checked_sub(k * STEP), base.checked_add(k * STEP)]
            .into_iter()
            .flatten()
        {
            // SAFETY: plain Win32 call; asking for a fixed address fails harmlessly when taken.
            let p = unsafe {
                VirtualAlloc(
                    addr as *const _,
                    STEP,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_EXECUTE_READWRITE,
                )
            };
            if !p.is_null() {
                return Some(p as usize);
            }
        }
    }
    None
}
