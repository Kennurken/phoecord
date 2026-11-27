//! Plugin entries in Phoenix's context menus (tasks 2.3.5, 2.3.11): the hook. Layout and the
//! pure steps are in [`crate::ui::context_menu`].
//!
//! Every context menu (message, user, channel, server) is built the same way: one entry record at
//! a time, appended to a `Vec` by a copy of the same small helper, on every frame while the menu
//! is open. Per menu:
//! - an inline detour (`retour`) on its append helper. The naked `push_entry_N` calls the
//!   original, then [`on_pushed`] with the helper's index, the `Vec` and its own return address;
//!   when that is the call right after the menu's native anchor entry, PhoeCord's records for that
//!   menu are appended through the original helper, so the client's own code grows the `Vec`.
//!   Several menus may share a helper copy; one detour per copy;
//! - the ids of what the menu is for: the user, channel and server menus read them from the
//!   anchor entry's own click handler (its closure captures them; [`IdSource::Closure`]). The
//!   message menu reads them from the build function's context: its `call` in the live caller is
//!   pointed at a stub next to the client (a `call rel32` reaches ±2 GiB) that jumps to
//!   [`build_entry`] ([`IdSource::BuildCall`]). No build function is patched: Sunbird already has
//!   a jump on the message menu's.
//!
//! A record's click handler is an `Rc<dyn Fn>`: PhoeCord's points at an `RcBox` it keeps for the
//! life of the process (counts so high they never reach zero, so the client never frees it) and a
//! vtable whose call slots are [`on_click`]. The click is handed to a [`MenuSink`] on a thread of
//! its own, never the client's UI thread.
//!
//! The entries are read from a list a background thread refreshes ([`refresh`]); a frame never
//! waits for the proxy.

use super::abi::Memory;
use super::detour::ProcessMemory;
use super::guard::{Breaker, LogFn};
use crate::hook::{HookError, HookSet};
use crate::ui::context_menu::{self as layout, ClosureIds, ITEM_SIZE};
use phoecord_core::MenuKind;
use phoecord_transport::menu::MenuClick;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

/// One plugin entry to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub plugin: String,
    pub entry: String,
    pub label: String,
    /// The menu it goes in (`message`, `user`, `channel`, `server`).
    pub menu: String,
}

/// Where the hook gets its entries and sends its clicks: the engine of an in-process launch, or
/// the proxy over the loader IPC.
pub trait MenuSink: Send + Sync {
    /// The entries of the plugins that run now. May block (IPC); called on a background thread.
    fn items(&self) -> Vec<Item>;
    /// A click on `plugin`'s `entry`, with the ids of its menu. Called on a thread of its own.
    fn click(&self, plugin: &str, entry: &str, click: MenuClick);
}

/// Where a menu's ids come from.
pub enum IdSource {
    /// The `call` (E8 rel32) of the build function, whose context holds the channel and message.
    BuildCall(usize),
    /// The anchor entry's click handler: the offsets of the ids in its closure.
    Closure(ClosureIds),
}

/// One menu to hook, as absolute addresses in the running client.
pub struct MenuTarget {
    pub kind: MenuKind,
    /// Entry of the menu's append helper.
    pub push: usize,
    /// Return address of the call that appends the native anchor entry.
    pub after_anchor: usize,
    pub ids: IdSource,
}

/// An entry PhoeCord made a record for once; kept for the life of the process, because the
/// client may still hold its label and handler.
struct Slot {
    plugin: String,
    entry: String,
    label: &'static str,
    menu: MenuKind,
    /// The `RcBox` of its handler: strong, weak, then the value (this slot's index).
    rc: usize,
    id: [u8; 16],
}

struct Menu {
    kind: MenuKind,
    /// Index of its helper detour.
    helper: usize,
    after_anchor: usize,
    closure: Option<ClosureIds>,
}

struct State {
    sink: Arc<dyn MenuSink>,
    log: LogFn,
    breaker: Breaker,
    menus: Vec<Menu>,
    slots: Mutex<Vec<Arc<Slot>>>,
    /// The slots to show, in order; written by [`refresh`], read on every menu frame.
    shown: RwLock<Vec<Arc<Slot>>>,
    /// The ids of the last menu of each kind built ([`kind_index`]).
    current: Mutex<[Option<MenuClick>; 4]>,
    vtable: usize,
}

static STATE: OnceLock<State> = OnceLock::new();
/// The build function of the message menu (the old target of the patched `call`).
static BUILDER: AtomicUsize = AtomicUsize::new(0);
/// The ids of the last message menu built (0: unknown).
static CHANNEL: AtomicU64 = AtomicU64::new(0);
static MESSAGE: AtomicU64 = AtomicU64::new(0);
static LOGGED: AtomicU32 = AtomicU32::new(0);
/// Menus (by [`kind_index`]) whose first append was logged.
static APPENDED: AtomicU32 = AtomicU32::new(0);

const LOG_FIRST_BUILD: u32 = 1;
const LOG_NO_TARGET: u32 = 2;
const LOG_READ: u32 = 4;
const LOG_FIRST_CLICK: u32 = 16;
const LOG_BUSY: u32 = 32;
const LOG_NO_IDS: u32 = 64;

/// How often the entry list is asked for again (plugins turned on or off, a language change).
const REFRESH: Duration = Duration::from_secs(3);
/// More than this many PhoeCord entries in one menu is a mistake; the rest are left out.
const MAX_SHOWN: usize = 32;
/// How many different helper copies the hook can detour.
pub const MAX_HELPERS: usize = 4;

fn once(state: &State, kind: u32, line: impl FnOnce() -> String) {
    if LOGGED.fetch_or(kind, Ordering::Relaxed) & kind == 0 {
        (state.log)(&line());
    }
}

fn kind_index(k: MenuKind) -> usize {
    MenuKind::ALL.iter().position(|x| *x == k).unwrap_or(0)
}

/// One naked detour per helper copy: each calls its own original, then [`on_pushed`] with its
/// index. `rsi`/`rdi` (callee-saved) keep the `Vec` and the return address across the original.
/// Alignment: two pushes and `0x38`.
macro_rules! helper_slot {
    ($entry:ident, $original:ident, $index:literal) => {
        static $original: AtomicUsize = AtomicUsize::new(0);

        #[unsafe(naked)]
        unsafe extern "C" fn $entry() {
            std::arch::naked_asm!(
                "push rsi",
                "push rdi",
                "sub rsp, 0x38",
                "mov rsi, rcx",
                "mov rdi, [rsp + 0x48]",
                "call qword ptr [rip + {original}]",
                concat!("mov ecx, ", $index),
                "mov rdx, rsi",
                "mov r8, rdi",
                "call {hook}",
                "add rsp, 0x38",
                "pop rdi",
                "pop rsi",
                "ret",
                hook = sym on_pushed,
                original = sym $original,
            );
        }
    };
}

helper_slot!(push_entry_0, ORIGINAL_0, "0");
helper_slot!(push_entry_1, ORIGINAL_1, "1");
helper_slot!(push_entry_2, ORIGINAL_2, "2");
helper_slot!(push_entry_3, ORIGINAL_3, "3");

type Entry = unsafe extern "C" fn();

fn helper(i: usize) -> Option<(Entry, &'static AtomicUsize)> {
    match i {
        0 => Some((push_entry_0 as Entry, &ORIGINAL_0)),
        1 => Some((push_entry_1 as Entry, &ORIGINAL_1)),
        2 => Some((push_entry_2 as Entry, &ORIGINAL_2)),
        3 => Some((push_entry_3 as Entry, &ORIGINAL_3)),
        _ => None,
    }
}

/// Prepares the detours on the menus' append helpers and patches the message menu's build call at
/// once; the detours are active when the set is installed.
///
/// # Safety
/// Every target must be the unique match of its locators in this client: `push` the entry of an
/// append helper (whole relocatable instructions first), `after_anchor` the instruction after one
/// of that helper's calls, a `BuildCall` a `call rel32` whose target is the message menu's build
/// function.
pub unsafe fn prepare(
    set: &mut HookSet,
    targets: Vec<MenuTarget>,
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
    let mut helpers: Vec<usize> = Vec::new();
    let mut menus = Vec::new();
    let mut build_call = None;
    for t in targets {
        let index = match helpers.iter().position(|h| *h == t.push) {
            Some(i) => i,
            None if helpers.len() < MAX_HELPERS => {
                helpers.push(t.push);
                helpers.len() - 1
            }
            None => {
                log(&format!(
                    "context menus: too many helper copies; the {} menu is left out",
                    t.kind.name()
                ));
                continue;
            }
        };
        let closure = match t.ids {
            IdSource::BuildCall(site) => {
                build_call = Some(site);
                None
            }
            IdSource::Closure(c) => Some(c),
        };
        menus.push(Menu {
            kind: t.kind,
            helper: index,
            after_anchor: t.after_anchor,
            closure,
        });
    }
    let state = State {
        sink,
        log,
        breaker: Breaker::new("context_menu"),
        menus,
        slots: Mutex::new(Vec::new()),
        shown: RwLock::new(Vec::new()),
        current: Mutex::new([None; 4]),
        vtable: vtable.as_ptr() as usize,
    };
    if STATE.set(state).is_err() {
        return Err(HookError::Detour(
            "the context menu hook is already prepared".into(),
        ));
    }
    for (i, push) in helpers.iter().enumerate() {
        let Some((entry, original)) = helper(i) else {
            break;
        };
        // SAFETY: forwarded from the caller's contract; the detour passes `rcx`/`rdx` to the
        // original and the helper returns nothing that matters.
        let hook = unsafe { set.add(*push as *const (), entry as *const ()) }?;
        original.store(hook.original() as usize, Ordering::Release);
    }
    if let Some(site) = build_call {
        // SAFETY: forwarded from the caller's contract for the build call.
        let builder = unsafe { redirect_call(site, build_entry as *const () as usize) }?;
        BUILDER.store(builder, Ordering::Release);
    }
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
    let mut shown: Vec<Arc<Slot>> = Vec::new();
    for item in items {
        let Some(menu) = MenuKind::from_name(&item.menu) else {
            continue;
        };
        if shown.iter().filter(|s| s.menu == menu).count() >= MAX_SHOWN {
            continue;
        }
        let existing = slots.iter().find(|s| {
            s.plugin == item.plugin
                && s.entry == item.entry
                && s.label == item.label
                && s.menu == menu
        });
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
                    menu,
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
    state.breaker.guard(
        &state.log,
        || (),
        || {
            once(state, LOG_FIRST_BUILD, || {
                "context menu: first message menu built".into()
            });
            let target = layout::read_target(ctx, |a| ProcessMemory.word(a));
            let (c, m) = target.map_or((0, 0), |t| (t.channel_id, t.message_id));
            if target.is_none() {
                once(state, LOG_NO_TARGET, || {
                    "context menu: the message menu's build context holds no channel/message ids"
                        .into()
                });
            }
            CHANNEL.store(c, Ordering::Relaxed);
            MESSAGE.store(m, Ordering::Relaxed);
        },
    );
}

/// After helper copy `helper` appended a record to `vec`, called from `ret`.
extern "C" fn on_pushed(helper: u32, vec: usize, ret: usize) {
    let Some(state) = STATE.get() else { return };
    let Some(menu) = state
        .menus
        .iter()
        .find(|m| m.after_anchor == ret && m.helper == helper as usize)
    else {
        return;
    };
    state
        .breaker
        .guard(&state.log, || (), || append(state, menu, vec));
}

/// The ids of `menu` from its anchor record (`template`), as the click its entries will send.
fn click_of(menu: &Menu, template: &[u8; ITEM_SIZE]) -> Option<MenuClick> {
    let Some(offsets) = &menu.closure else {
        let (c, m) = (
            CHANNEL.load(Ordering::Relaxed),
            MESSAGE.load(Ordering::Relaxed),
        );
        return (c != 0 && m != 0).then_some(MenuClick::Message {
            channel_id: c,
            message_id: m,
        });
    };
    let rc = usize::from_le_bytes(
        template[layout::HANDLER_RC..layout::HANDLER_RC + 8]
            .try_into()
            .ok()?,
    );
    let ids = layout::read_closure(rc, offsets, |a| ProcessMemory.word(a));
    match menu.kind {
        MenuKind::User => Some(MenuClick::User {
            user_id: ids.user?,
            guild_id: ids.guild,
            channel_id: ids.channel,
        }),
        MenuKind::Channel => Some(MenuClick::Channel {
            channel_id: ids.channel?,
            guild_id: ids.guild,
        }),
        MenuKind::Server => Some(MenuClick::Server {
            guild_id: ids.guild?,
        }),
        _ => None,
    }
}

fn append(state: &State, menu: &Menu, vec: usize) {
    // A frame never waits for `refresh`: with the list being replaced, this frame shows none.
    let Ok(shown) = state.shown.try_read() else {
        return once(state, LOG_BUSY, || {
            "context menu: the entry list was being refreshed; one frame without PhoeCord entries"
                .into()
        });
    };
    if !shown.iter().any(|s| s.menu == menu.kind) {
        return;
    }
    let mem = ProcessMemory;
    let (Some(ptr), Some(len)) = (mem.word(vec + 8), mem.word(vec + 0x10)) else {
        return once(state, LOG_READ, || {
            "context menu: the entry Vec is not readable".into()
        });
    };
    let mut template = [0u8; ITEM_SIZE];
    if len == 0 || !mem.read(ptr + (len - 1) * ITEM_SIZE, &mut template) {
        return once(state, LOG_READ, || {
            "context menu: the native entry record is not readable".into()
        });
    }
    let Some(click) = click_of(menu, &template) else {
        return once(state, LOG_NO_IDS, || {
            format!(
                "context menu: the {} menu's anchor holds no ids; its PhoeCord entries are left out",
                menu.kind.name()
            )
        });
    };
    if let Ok(mut current) = state.current.lock() {
        current[kind_index(menu.kind)] = Some(click);
    }
    let Some((_, original)) = helper(menu.helper) else {
        return;
    };
    let original = original.load(Ordering::Acquire);
    if original == 0 {
        return;
    }
    // SAFETY: `original` is the relocated append helper `push(vec: &mut Vec<Item>, item: *const
    // Item)`, which copies `ITEM_SIZE` bytes from `item` (growing the Vec through the client's
    // allocator first when it is full). `vec` is the Vec it was just called with, on this thread.
    let push: unsafe extern "C" fn(usize, *const u8) = unsafe { std::mem::transmute(original) };
    let mut n = 0;
    for slot in shown.iter().filter(|s| s.menu == menu.kind) {
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
        n += 1;
    }
    let bit = 1 << kind_index(menu.kind);
    if APPENDED.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
        // The labels of the section the entries went into, to tell a wrong anchor from a right one.
        let labels: Vec<String> = (0..len.min(24))
            .map(|i| record_label(ptr + i * ITEM_SIZE).unwrap_or_else(|| "-".into()))
            .collect();
        (state.log)(&format!(
            "context menu: {n} PhoeCord entr(y/ies) appended to a {} menu section [{}]",
            menu.kind.name(),
            labels.join(" | ")
        ));
    }
}

/// The label of the record at `at`, when it has a readable one.
fn record_label(at: usize) -> Option<String> {
    let mem = ProcessMemory;
    let (p, n) = (
        mem.word(at + layout::LABEL_PTR)?,
        mem.word(at + layout::LABEL_LEN)?,
    );
    if n == 0 || n > 80 {
        return None;
    }
    let mut b = vec![0u8; n];
    mem.read(p, &mut b)
        .then(|| String::from_utf8_lossy(&b).into_owned())
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
                format!("context menu: first click ({}/{})", slot.plugin, slot.entry)
            });
            let click = state
                .current
                .lock()
                .ok()
                .and_then(|c| c[kind_index(slot.menu)]);
            let Some(click) = click else {
                return once(state, LOG_NO_TARGET, || {
                    "context menu: a click without the menu's ids; not sent".into()
                });
            };
            let sink = state.sink.clone();
            let _ = std::thread::Builder::new()
                .name("phoecord-menu-click".into())
                .spawn(move || sink.click(&slot.plugin, &slot.entry, click));
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
