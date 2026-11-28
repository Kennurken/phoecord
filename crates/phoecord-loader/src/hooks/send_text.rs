//! What the outgoing gateway hook on `send_text` (task 2.2.4) does with one frame, on plain memory.
//!
//! Every client → server text frame (the encoder's frames and `IDENTIFY`, resume and the
//! heartbeat alike) ends in one function of the connection loop, `send_text(out, session,
//! &String)`, which turns the frame `String` into a websocket message and frees it (private recon
//! notes, 2.2.4 send path, 2026-11-27). The detour runs on entry, **before** the original:
//!
//! - **pass:** the original runs on the frame as it is;
//! - **changed:** the `String` at `frame` gets a new buffer from the client's allocator and the old
//!   one is freed; the original sends the new text and frees it;
//! - **dropped:** the original is not called. Its caller hands it the frame to own, so the hook
//!   frees the buffer, leaves an empty `String` in the record (`{0, 1, 0}`, freed by nobody), and
//!   writes [`NOTHING_TO_SEND`] into the first word of `out`: the loop's "no pending write" value.
//!
//! Without the client's allocator a frame cannot be freed or replaced safely, so it is sent as it
//! is. The detour itself (`hooks::gateway_send_text`) is Windows-only; this part is not, so it is
//! tested everywhere.

use super::abi::Memory;
use super::gateway::GatewayAction;

/// The longest frame read; Discord's limit for a client frame is 4096 bytes, so anything past
/// this is a wrong reading.
pub const MAX_FRAME: usize = 64 * 1024;

/// The first word of `send_text`'s result that the connection loop reads as "nothing to send"
/// (`cmp rax, -1; je skip` in the loop; `-1 | 2` stays `-1`).
pub const NOTHING_TO_SEND: usize = usize::MAX;

/// The client's heap: `__rust_alloc` / `__rust_dealloc` with alignment 1, and writes.
pub trait Heap {
    /// `len` bytes, or `None` when the allocator refused.
    fn alloc(&self, len: usize) -> Option<usize>;
    /// Frees a buffer of capacity `cap` that `alloc` (or the client) made.
    fn free(&self, ptr: usize, cap: usize);
    fn write(&self, addr: usize, bytes: &[u8]);
}

/// What happened to one frame; [`Outcome::skip`] says whether the original must not run.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The record at `frame` is not a frame `String`; sent untouched.
    Unreadable,
    Passed,
    Replaced,
    Dropped,
    /// A plugin changed (`dropped: false`) or dropped the frame, but this build has no allocator;
    /// sent unchanged.
    NoAllocator {
        dropped: bool,
    },
    /// The client's allocator refused the new text; sent unchanged.
    AllocFailed,
}

impl Outcome {
    pub fn skip(&self) -> bool {
        matches!(self, Outcome::Dropped)
    }
}

/// The `String` record `{cap, ptr, len}` at `at`: (capacity, pointer, text), or `None` when it
/// does not look like a frame.
pub fn read_string(mem: &dyn Memory, at: usize) -> Option<(usize, usize, Vec<u8>)> {
    let (cap, ptr, len) = (mem.word(at)?, mem.word(at + 8)?, mem.word(at + 16)?);
    if ptr == 0 || len == 0 || len > cap || len > MAX_FRAME {
        return None;
    }
    let mut text = vec![0u8; len];
    mem.read(ptr, &mut text).then_some((cap, ptr, text))
}

fn record(cap: usize, ptr: usize, len: usize) -> [u8; 24] {
    let mut out = [0u8; 24];
    for (i, w) in [cap, ptr, len].into_iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&w.to_le_bytes());
    }
    out
}

/// One call of `send_text`: `out` is its result (`rcx`), `frame` the `String` record (`r8`).
/// `verdict` is the plugin chain's answer on the frame's JSON text.
pub fn handle(
    mem: &dyn Memory,
    heap: Option<&dyn Heap>,
    out: usize,
    frame: usize,
    verdict: impl FnOnce(&[u8]) -> GatewayAction,
) -> Outcome {
    let Some((cap, ptr, text)) = read_string(mem, frame) else {
        return Outcome::Unreadable;
    };
    match verdict(&text) {
        // `Rename` comes from incoming dispatches only, never from an outgoing frame.
        GatewayAction::Pass | GatewayAction::Rename { .. } => Outcome::Passed,
        GatewayAction::Replace(new) => {
            let Some(heap) = heap else {
                return Outcome::NoAllocator { dropped: false };
            };
            if new.is_empty() {
                return Outcome::Passed;
            }
            let Some(p) = heap.alloc(new.len()) else {
                return Outcome::AllocFailed;
            };
            heap.write(p, &new);
            heap.write(frame, &record(new.len(), p, new.len()));
            if cap > 0 {
                heap.free(ptr, cap);
            }
            Outcome::Replaced
        }
        GatewayAction::Drop => {
            let Some(heap) = heap else {
                return Outcome::NoAllocator { dropped: true };
            };
            // An empty `String` first, so nothing is left pointing at the freed buffer.
            heap.write(frame, &record(0, 1, 0));
            if cap > 0 {
                heap.free(ptr, cap);
            }
            heap.write(out, &NOTHING_TO_SEND.to_le_bytes());
            Outcome::Dropped
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// Byte-addressed memory with an allocator that hands out fresh ranges and records frees.
    #[derive(Default)]
    struct Fake {
        bytes: RefCell<BTreeMap<usize, u8>>,
        next: RefCell<usize>,
        freed: RefCell<Vec<(usize, usize)>>,
        refuse: bool,
    }

    impl Fake {
        fn with_frame(text: &[u8], cap: usize) -> Fake {
            let f = Fake {
                next: RefCell::new(0x10_000),
                ..Fake::default()
            };
            f.write(0x900, text);
            f.write(FRAME, &record(cap, 0x900, text.len()));
            f.write(OUT, &[0u8; 32]);
            f
        }
        fn words(&self, at: usize) -> [usize; 3] {
            [0, 1, 2].map(|i| self.word(at + 8 * i).unwrap())
        }
        fn text(&self, at: usize) -> Vec<u8> {
            read_string(self, at).unwrap().2
        }
    }

    impl Memory for Fake {
        fn read(&self, addr: usize, buf: &mut [u8]) -> bool {
            let m = self.bytes.borrow();
            for (i, b) in buf.iter_mut().enumerate() {
                match m.get(&(addr + i)) {
                    Some(v) => *b = *v,
                    None => return false,
                }
            }
            true
        }
    }

    impl Heap for Fake {
        fn alloc(&self, len: usize) -> Option<usize> {
            if self.refuse {
                return None;
            }
            let p = *self.next.borrow();
            *self.next.borrow_mut() += len.max(1);
            Some(p)
        }
        fn free(&self, ptr: usize, cap: usize) {
            self.freed.borrow_mut().push((ptr, cap));
        }
        fn write(&self, addr: usize, bytes: &[u8]) {
            let mut m = self.bytes.borrow_mut();
            for (i, b) in bytes.iter().enumerate() {
                m.insert(addr + i, *b);
            }
        }
    }

    const OUT: usize = 0x100;
    const FRAME: usize = 0x200;
    const PRESENCE: &[u8] = br#"{"op":3,"d":{"status":"online"}}"#;

    #[test]
    fn a_passed_frame_is_left_alone() {
        let f = Fake::with_frame(PRESENCE, 64);
        let mut seen = Vec::new();
        let o = handle(&f, Some(&f), OUT, FRAME, |t| {
            seen = t.to_vec();
            GatewayAction::Pass
        });
        assert_eq!(o, Outcome::Passed);
        assert!(!o.skip());
        assert_eq!(seen, PRESENCE);
        assert_eq!(f.words(FRAME), [64, 0x900, PRESENCE.len()]);
        assert!(f.freed.borrow().is_empty());
    }

    #[test]
    fn a_dropped_frame_is_freed_once_and_the_loop_sees_nothing_to_send() {
        let f = Fake::with_frame(PRESENCE, 64);
        let o = handle(&f, Some(&f), OUT, FRAME, |_| GatewayAction::Drop);
        assert_eq!(o, Outcome::Dropped);
        assert!(o.skip(), "the original must not run");
        assert_eq!(*f.freed.borrow(), [(0x900, 64)], "the old buffer, once");
        assert_eq!(f.words(FRAME), [0, 1, 0], "an empty String nobody frees");
        assert_eq!(f.word(OUT), Some(NOTHING_TO_SEND));
    }

    #[test]
    fn a_changed_frame_gets_a_new_buffer_and_the_old_one_is_freed() {
        let f = Fake::with_frame(PRESENCE, 64);
        let new = br#"{"op":3,"d":{"status":"idle"}}"#.to_vec();
        let o = handle(&f, Some(&f), OUT, FRAME, |_| {
            GatewayAction::Replace(new.clone())
        });
        assert_eq!(o, Outcome::Replaced);
        assert!(!o.skip(), "the original sends the new text");
        assert_eq!(f.text(FRAME), new);
        assert_eq!(f.words(FRAME)[1], 0x10_000, "the client's allocator");
        assert_eq!(*f.freed.borrow(), [(0x900, 64)]);
        assert_eq!(
            f.word(OUT),
            Some(0),
            "the result is the original's to write"
        );
    }

    #[test]
    fn without_an_allocator_nothing_is_changed_or_dropped() {
        let f = Fake::with_frame(PRESENCE, 64);
        assert_eq!(
            handle(&f, None, OUT, FRAME, |_| GatewayAction::Drop),
            Outcome::NoAllocator { dropped: true }
        );
        assert_eq!(
            handle(&f, None, OUT, FRAME, |_| GatewayAction::Replace(
                b"{}".to_vec()
            )),
            Outcome::NoAllocator { dropped: false }
        );
        assert_eq!(f.words(FRAME), [64, 0x900, PRESENCE.len()]);
        assert_eq!(f.word(OUT), Some(0));
    }

    #[test]
    fn a_refused_allocation_sends_the_frame_unchanged() {
        let mut f = Fake::with_frame(PRESENCE, 64);
        f.refuse = true;
        let o = handle(&f, Some(&f), OUT, FRAME, |_| {
            GatewayAction::Replace(b"{}".to_vec())
        });
        assert_eq!(o, Outcome::AllocFailed);
        assert_eq!(f.text(FRAME), PRESENCE);
        assert!(f.freed.borrow().is_empty());
    }

    #[test]
    fn a_record_that_is_not_a_frame_is_not_touched() {
        let f = Fake::with_frame(PRESENCE, 4);
        let mut asked = false;
        let o = handle(&f, Some(&f), OUT, FRAME, |_| {
            asked = true;
            GatewayAction::Drop
        });
        assert_eq!(o, Outcome::Unreadable, "length over capacity");
        assert!(!asked, "plugins never see a wrong reading");
        assert!(f.freed.borrow().is_empty());
        let f = Fake::default();
        f.write(FRAME, &record(16, 0, 16));
        assert_eq!(
            handle(&f, Some(&f), OUT, FRAME, |_| GatewayAction::Drop),
            Outcome::Unreadable,
            "null pointer"
        );
    }
}
