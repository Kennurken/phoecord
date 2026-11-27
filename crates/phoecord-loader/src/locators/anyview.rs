//! Locator for GPUI's central `AnyView` construction dispatcher (task 2.3.10).
//!
//! `docs/re/gpui-lifecycle.md` names `FUN_148784dd0` as the "central `AnyView`
//! registration/creation dispatcher (takes the type name `&str`, size, alignment and a pointer
//! to the render vtable)", from a pass whose own build was not recorded. Static read-only
//! analysis of the installed Phoenix exe (`0.1.0.2208`, same hash as `phoenix-recon.md`'s Part 1)
//! confirms that note's addresses *are* this exact build: several of its RVAs (this one among
//! them) resolve to real, `.pdata`-bounded function starts here, and six independent `.text`
//! sites that construct six different concrete view types (found via their own type-name
//! strings, `phoecord-recon`, 2026-11-27) all call this one address right after loading their
//! type's name and a vtable pointer — exactly the signature the note describes.
//!
//! Unlike [`crate::locators::gateway`] and [`crate::locators::theme`], this one needs no second
//! fact: the bare prologue is already unique in `.text`.
//!
//! What this gives: one function every `AnyView` (every message row, every panel, every view
//! GPUI ever builds) passes through on construction, with the type's own render vtable pointer
//! as an argument — the generic point task 2.3.10 asks for ("the one wrap around a GPUI view's
//! `render` that every on-screen part builds on"), not a per-view-type anchor. Wrapping this
//! call (swap the incoming vtable pointer for one of our own, whose `render` calls the original
//! then adds plugin accessories) is the design 2.3.10 would build a hook on; not done here.
//!
//! Checked against Phoenix `0.1.0.2208` only; not yet verified on a second build (task 2.2.9).

use super::{Found, Image, LocatorError, exactly_one};
use crate::aob::Pattern;

/// Prologue of `FUN_148784dd0` (`docs/re/gpui-lifecycle.md`), RVA `0x8784dd0` in the checked
/// build. Matches exactly once in `.text` — no cross-reference needed.
const DISPATCHER_AOB: &str = "55 41 57 41 56 41 55 41 54 56 57 53 48 81 EC 88 00 00 00 \
     48 8D AC 24 80 00 00 00 48 C7 45 00 FE FF FF FF 4C 89 4D D8 4D 89 C6 48 FF 81 D8 05 00";

/// The `AnyView` construction dispatcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnyViewDispatcher {
    /// Entry RVA, for an inline hook.
    pub rva: u32,
}

/// Locates the dispatcher.
pub fn find(image: &Image<'_>) -> Result<Found<AnyViewDispatcher>, LocatorError> {
    let pattern = Pattern::parse(DISPATCHER_AOB).expect("locator AOB is a fixed, valid pattern");
    let matches = pattern
        .find_all(image.text.bytes)
        .map(|off| image.text.rva + off as u32);
    let rva = exactly_one("AnyView construction dispatcher", matches)?;
    Ok(Found::new(
        AnyViewDispatcher { rva },
        "masked AOB on the dispatcher's own prologue, unique alone (docs/re/gpui-lifecycle.md, \
         FUN_148784dd0; cross-checked against 6 independent view-construction call sites, \
         phoecord-recon, 2026-11-27)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locators::Region;

    fn pattern_bytes() -> Vec<u8> {
        DISPATCHER_AOB
            .split_whitespace()
            .map(|t| {
                if t == "??" {
                    0x90
                } else {
                    u8::from_str_radix(t, 16).unwrap()
                }
            })
            .collect()
    }

    fn image(text: Vec<u8>) -> Image<'static> {
        Image {
            text: Region {
                rva: 0x1000,
                bytes: Box::leak(text.into_boxed_slice()),
            },
            rdata: Region { rva: 0, bytes: &[] },
            functions: vec![],
            image_base: 0,
        }
    }

    #[test]
    fn finds_the_dispatcher() {
        let bytes = pattern_bytes();
        let mut text = vec![0xCCu8; 0x80];
        text[0x20..0x20 + bytes.len()].copy_from_slice(&bytes);
        let found = find(&image(text)).unwrap();
        assert_eq!(found.value.rva, 0x1020);
    }

    #[test]
    fn missing_is_not_guessed() {
        let text = vec![0xCCu8; 0x80];
        assert_eq!(
            find(&image(text)),
            Err(LocatorError::Missing {
                what: "AnyView construction dispatcher".into()
            })
        );
    }

    #[test]
    fn two_copies_are_ambiguous_not_a_guess() {
        let bytes = pattern_bytes();
        let mut text = vec![0xCCu8; 0x200];
        text[0x20..0x20 + bytes.len()].copy_from_slice(&bytes);
        text[0x120..0x120 + bytes.len()].copy_from_slice(&bytes);
        assert!(matches!(
            find(&image(text)),
            Err(LocatorError::Ambiguous { count: 2, .. })
        ));
    }
}
