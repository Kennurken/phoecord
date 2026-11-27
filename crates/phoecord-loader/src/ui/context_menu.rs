//! Plugin entries in Phoenix's context menus (tasks 2.3.5, 2.3.11): the pure part.
//!
//! Found live on Phoenix 0.1.0.2208 (private recon notes, 2.3.5): the right-click menu and the
//! "..." button share one build function, which runs on every frame while a menu is open. Each
//! entry is built into a record of [`ITEM_SIZE`] bytes and appended to a `Vec` by a small shared
//! helper (grow if full, copy, `len += 1`). A record holds its label as a `SharedString` and its
//! click handler as an `Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>`, so every entry carries its
//! own handler and no native dispatcher is involved. The build function's context holds the
//! channel and the message the menu is for.
//!
//! So the hook (`hooks::context_menu`, Windows) appends, right after the native "Copy Message
//! Link" entry, one record per plugin entry: a copy of that native record with PhoeCord's label,
//! element id and handler ([`build_record`]). This module has the layout and the pure steps, tested
//! on plain bytes.

/// Size of one entry record in the menu's `Vec`.
pub const ITEM_SIZE: usize = 0x1f8;
/// The handler: `Rc<dyn Fn>` data pointer (to the `RcBox`) and vtable.
pub const HANDLER_RC: usize = 0x10;
pub const HANDLER_VTABLE: usize = 0x18;
/// The label `SharedString`: a tag word (static string), then pointer and length.
pub const LABEL_PTR: usize = 0xb8;
pub const LABEL_LEN: usize = 0xc0;
/// The element id, inline: a length byte and up to [`ID_MAX`] bytes, 16 bytes in all.
pub const ELEMENT_ID: usize = 0xc8;
pub const ID_MAX: usize = 15;
/// Offsets of the channel and message ids (`u64`) in the build function's context (`rdx`).
pub const CONTEXT_CHANNEL: usize = 0x80;
pub const CONTEXT_MESSAGE: usize = 0x88;

/// Where the ids sit in a native anchor entry's click handler: offsets into its closure (the value
/// of its `RcBox`, at `+0x10`). Found live on Phoenix 0.1.0.2208 (2.3.11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClosureIds {
    pub user: Option<usize>,
    pub guild: Option<usize>,
    pub channel: Option<usize>,
}

/// The user menu's "Block" entry: server, channel and the user. ("Profile" holds the same ids but
/// sits alone in its section, where an appended entry is not drawn.)
pub const USER_ANCHOR: ClosureIds = ClosureIds {
    user: Some(0x30),
    guild: Some(0x8),
    channel: Some(0x18),
};
/// The channel menu's "Copy Link" entry: the channel. ("Mark As Read" sits alone in its section.)
pub const CHANNEL_ANCHOR: ClosureIds = ClosureIds {
    user: None,
    guild: None,
    channel: Some(0x30),
};
/// The server menu's "Mark As Read" entry: the server.
pub const SERVER_ANCHOR: ClosureIds = ClosureIds {
    user: None,
    guild: Some(0x0),
    channel: None,
};

/// The ids a closure holds, read through `word`; an id that is unreadable or does not look like a
/// snowflake is `None` (a user menu in a DM has no server).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReadIds {
    pub user: Option<u64>,
    pub guild: Option<u64>,
    pub channel: Option<u64>,
}

/// The ids in the closure of the handler whose `RcBox` is at `rc`.
pub fn read_closure(rc: usize, at: &ClosureIds, word: impl Fn(usize) -> Option<usize>) -> ReadIds {
    let value = rc.checked_add(0x10);
    let id = |off: Option<usize>| {
        let v = word(value?.checked_add(off?)?)? as u64;
        plausible_snowflake(v).then_some(v)
    };
    ReadIds {
        user: id(at.user),
        guild: id(at.guild),
        channel: id(at.channel),
    }
}

/// The ids of the message a menu is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuTarget {
    pub channel_id: u64,
    pub message_id: u64,
}

/// Whether `v` looks like a Discord snowflake made between 2020 and 2027, so a wrong context
/// offset is refused instead of sent to a plugin.
pub fn plausible_snowflake(v: u64) -> bool {
    const FROM: u64 = 157_766_400_000; // 2020-01-01, in ms since Discord's epoch
    const TO: u64 = 378_691_200_000; // 2027-01-01
    (FROM..TO).contains(&(v >> 22))
}

/// The channel and message ids from the build function's context at `ctx`, read through `word`;
/// `None` when either is unreadable or does not look like a snowflake.
pub fn read_target(ctx: usize, word: impl Fn(usize) -> Option<usize>) -> Option<MenuTarget> {
    let channel_id = word(ctx.checked_add(CONTEXT_CHANNEL)?)? as u64;
    let message_id = word(ctx.checked_add(CONTEXT_MESSAGE)?)? as u64;
    (plausible_snowflake(channel_id) && plausible_snowflake(message_id)).then_some(MenuTarget {
        channel_id,
        message_id,
    })
}

/// The 16 inline bytes of an element id: its length, then the bytes, zero padded. `None` when the
/// id is empty or longer than [`ID_MAX`].
pub fn inline_id(id: &str) -> Option<[u8; 16]> {
    if id.is_empty() || id.len() > ID_MAX {
        return None;
    }
    let mut out = [0u8; 16];
    out[0] = id.len() as u8;
    out[1..=id.len()].copy_from_slice(id.as_bytes());
    Some(out)
}

/// The element id of PhoeCord's `n`-th menu entry (unique next to the native ones).
pub fn element_id(n: usize) -> String {
    format!("phoecord-{n}")
}

/// PhoeCord's record: `template` (the native record just appended) with PhoeCord's handler
/// (`rc`, `vtable`), label (`label_ptr`, `label_len`: a string that lives as long as the process)
/// and element id. Everything else of the native record is kept as it is.
pub fn build_record(
    template: &[u8; ITEM_SIZE],
    rc: usize,
    vtable: usize,
    label_ptr: usize,
    label_len: usize,
    id: &[u8; 16],
) -> [u8; ITEM_SIZE] {
    let mut r = *template;
    let put = |r: &mut [u8; ITEM_SIZE], at: usize, v: usize| {
        r[at..at + 8].copy_from_slice(&(v as u64).to_le_bytes());
    };
    put(&mut r, HANDLER_RC, rc);
    put(&mut r, HANDLER_VTABLE, vtable);
    put(&mut r, LABEL_PTR, label_ptr);
    put(&mut r, LABEL_LEN, label_len);
    r[ELEMENT_ID..ELEMENT_ID + 16].copy_from_slice(id);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word_at(r: &[u8], at: usize) -> usize {
        u64::from_le_bytes(r[at..at + 8].try_into().unwrap()) as usize
    }

    #[test]
    fn a_record_gets_our_handler_label_and_id_and_keeps_the_rest() {
        let mut template = [0u8; ITEM_SIZE];
        for (i, b) in template.iter_mut().enumerate() {
            *b = i as u8;
        }
        let id = inline_id("phoecord-0").unwrap();
        let r = build_record(&template, 0x1000, 0x2000, 0x3000, 18, &id);
        assert_eq!(word_at(&r, HANDLER_RC), 0x1000);
        assert_eq!(word_at(&r, HANDLER_VTABLE), 0x2000);
        assert_eq!(word_at(&r, LABEL_PTR), 0x3000);
        assert_eq!(word_at(&r, LABEL_LEN), 18);
        assert_eq!(&r[ELEMENT_ID..ELEMENT_ID + 16], &id);
        // Untouched: the tag before the handler, the label's tag word, the tail.
        assert_eq!(r[..HANDLER_RC], template[..HANDLER_RC]);
        assert_eq!(r[0xb0..LABEL_PTR], template[0xb0..LABEL_PTR]);
        assert_eq!(r[ELEMENT_ID + 16..], template[ELEMENT_ID + 16..]);
    }

    #[test]
    fn closure_ids_are_read_from_the_handler_value() {
        // The owner's live check: server, channel and user in "Block"'s closure.
        let rc = 0x9000;
        let (g, c, u) = (
            1324239354154975252u64,
            1533312669321531572u64,
            936039926477242468u64,
        );
        let mem = move |a: usize| match a - rc - 0x10 {
            0x8 => Some(g as usize),
            0x18 => Some(c as usize),
            0x30 => Some(u as usize),
            _ => Some(0),
        };
        assert_eq!(
            read_closure(rc, &USER_ANCHOR, mem),
            ReadIds {
                user: Some(u),
                guild: Some(g),
                channel: Some(c)
            }
        );
        // In a DM the server slot holds no snowflake.
        let dm = move |a: usize| {
            if a - rc - 0x10 == 0x30 {
                Some(u as usize)
            } else {
                Some(0)
            }
        };
        let ids = read_closure(rc, &USER_ANCHOR, dm);
        assert_eq!((ids.user, ids.guild), (Some(u), None));
        assert_eq!(
            read_closure(rc, &SERVER_ANCHOR, mem).guild,
            None,
            "0x0 holds no id here"
        );
    }

    #[test]
    fn inline_ids_are_length_prefixed_and_bounded() {
        let id = inline_id("phoecord").unwrap();
        assert_eq!(id[0], 8);
        assert_eq!(&id[1..9], b"phoecord");
        assert!(id[9..].iter().all(|b| *b == 0));
        assert!(inline_id("").is_none());
        assert!(inline_id(&"x".repeat(16)).is_none());
        assert!(inline_id(&element_id(99_999)).is_some());
    }

    #[test]
    fn the_target_comes_from_the_context_and_is_checked() {
        let ctx = 0x5000;
        let mem = |channel: u64, message: u64| {
            move |a: usize| match a - ctx {
                CONTEXT_CHANNEL => Some(channel as usize),
                CONTEXT_MESSAGE => Some(message as usize),
                _ => None,
            }
        };
        // The owner's ids from the live check.
        let (c, m) = (1219803441186996275, 1574877017390981191);
        assert_eq!(
            read_target(ctx, mem(c, m)),
            Some(MenuTarget {
                channel_id: c,
                message_id: m
            })
        );
        assert_eq!(read_target(ctx, mem(c, 0x20)), None, "not a snowflake");
        assert_eq!(read_target(ctx, |_| None), None, "unreadable");
    }
}
