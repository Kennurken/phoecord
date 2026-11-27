//! Locator for Phoenix's theme/palette global (task 2.3.1).
//!
//! `docs/research/gpui.md` predicted the shape (`cx.global::<T>()`, panicking `"no state of type
//! {} exists"` on a miss) from Zed/GPUI's own pattern, without a Phoenix-specific name. Static
//! read-only analysis of the installed Phoenix exe (`0.1.0.2208`, same build and hash as the
//! private recon notes) found it: `mana_gpui::theme::ManaTheme` — read through Phoenix's own
//! vendored GPUI fork, `vendor/gpui-pre/src/app.rs` (recon PR, `phoecord-recon`, 2026-11-27).
//!
//! The bare prologue of its accessor is **not unique**: it matches 12 times in `.text`, one per
//! monomorphized `App::global::<T>()` — a generic, byte-identical template shared by every
//! `Global` type, differing only in which type's name the cold panic tail cites. So, same as
//! [`crate::locators::gateway`], the prologue AOB alone cannot be the whole locator: [`find`]
//! intersects it with a `.text` reference to `ManaTheme`'s own name string, the one fact that
//! does pick a single candidate.
//!
//! **This gives the accessor function, not a fixed struct offset.** The accessor's own
//! `[reg+0x4d0] == 0` presence check (seen in the one confirmed instance) reads a per-type
//! hash-table slot resolved earlier in the function, not a static address — the same shape, and
//! the same reason, `gateway::find`'s target has no fixed table address either (2.1.6). An inline
//! hook on this function's entry/return is 2.3.1's path forward, not a memory patch.
//!
//! Checked against Phoenix `0.1.0.2208` only; not yet verified on a second build (task 2.2.9).

use super::{CallIndex, Found, Image, LocatorError, exactly_one};
use crate::aob::Pattern;

/// Prologue of one monomorphized `App::global::<T>()`, through its presence check and
/// conditional jump to the cold panic tail. The `movdqu` operand (a build-specific constant, not
/// type-specific) is masked. Matches 12 times alone — every `Global` type shares this template.
const ACCESSOR_AOB: &str = "41 57 41 56 41 55 41 54 56 57 53 48 81 EC 80 00 00 00 \
     66 44 0F 7F 44 24 70 66 0F 7F 7C 24 60 66 0F 7F 74 24 50 \
     F3 0F 6F 05 ?? ?? ?? ?? 66 0F 7F 44 24 40 48 83 B9 D0 04 00 00 00";

/// `ManaTheme`'s own type name, as `std::any::type_name::<ManaTheme>()` embeds it — the one fact
/// that tells the 12 candidates apart.
const TYPE_NAME: &str = "mana_gpui::theme::ManaTheme";

/// Phoenix's theme/palette global accessor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManaTheme {
    /// Entry RVA of `App::global::<ManaTheme>()`, for an inline hook.
    pub accessor_rva: u32,
}

/// Locates the accessor: the prologue match that also refers to `ManaTheme`'s own name string.
pub fn find(image: &Image<'_>) -> Result<Found<ManaTheme>, LocatorError> {
    let pattern = Pattern::parse(ACCESSOR_AOB).expect("locator AOB is a fixed, valid pattern");
    let prologue_matches: Vec<u32> = pattern
        .find_all(image.text.bytes)
        .map(|off| image.text.rva + off as u32)
        .collect();
    let type_name_rva = image.find_str(TYPE_NAME)?;
    let index = CallIndex::build(image);
    let loaders = index.loaders(image, type_name_rva);
    let accessor_rva = exactly_one(
        "ManaTheme global accessor",
        prologue_matches.into_iter().filter(|m| loaders.contains(m)),
    )?;
    Ok(Found::new(
        ManaTheme { accessor_rva },
        "masked AOB on one monomorphized App::global::<T>()'s prologue, confirmed by a \
         reference to ManaTheme's own type-name string (phoecord-recon, 2026-11-27)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locators::Region;

    fn pattern_bytes() -> Vec<u8> {
        ACCESSOR_AOB
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

    /// `lea rax, [rip + disp32]` at `at` (text RVA `text_rva + at`), pointing at `target_rva`.
    fn lea(text: &mut [u8], text_rva: u32, at: usize, target_rva: u32) {
        text[at..at + 3].copy_from_slice(&[0x48, 0x8D, 0x05]);
        let end = text_rva + at as u32 + 7;
        let rel = target_rva as i64 - end as i64;
        text[at + 3..at + 7].copy_from_slice(&(rel as i32).to_le_bytes());
    }

    /// Two copies of the bare accessor prologue (the real ambiguity: 12 on the real exe), 0x200
    /// apart; only the one at `with_ref` also loads [`TYPE_NAME`]'s address, if given.
    fn image_with_two_candidates(with_ref: Option<usize>) -> Image<'static> {
        let bytes = pattern_bytes();
        let mut text = vec![0xCCu8; 0x400];
        text[0x20..0x20 + bytes.len()].copy_from_slice(&bytes);
        text[0x220..0x220 + bytes.len()].copy_from_slice(&bytes);
        if let Some(at) = with_ref {
            lea(&mut text, 0x1000, at, 0x8000);
        }
        let rdata = TYPE_NAME.as_bytes().to_vec();
        Image {
            text: Region {
                rva: 0x1000,
                bytes: Box::leak(text.into_boxed_slice()),
            },
            rdata: Region {
                rva: 0x8000,
                bytes: Box::leak(rdata.into_boxed_slice()),
            },
            functions: vec![(0x1020, 0x1220), (0x1220, 0x1420)],
            image_base: 0,
        }
    }

    #[test]
    fn two_bare_prologue_matches_are_disambiguated_by_the_type_name_reference() {
        // The second candidate (0x1220) is the real accessor: it alone loads the type-name
        // string, reproducing the real exe's 12-candidate ambiguity in miniature.
        let img = image_with_two_candidates(Some(0x260));
        let found = find(&img).unwrap();
        assert_eq!(found.value.accessor_rva, 0x1220);
    }

    #[test]
    fn the_prologue_alone_is_ambiguous_and_not_guessed() {
        let img = image_with_two_candidates(None);
        assert_eq!(
            find(&img),
            Err(LocatorError::Missing {
                what: "ManaTheme global accessor".into()
            })
        );
    }

    #[test]
    fn missing_type_name_is_not_guessed() {
        let bytes = pattern_bytes();
        let mut text = vec![0xCCu8; 0x80];
        text[0x20..0x20 + bytes.len()].copy_from_slice(&bytes);
        let img = Image {
            text: Region {
                rva: 0x1000,
                bytes: Box::leak(text.into_boxed_slice()),
            },
            rdata: Region {
                rva: 0x8000,
                bytes: b"no type name here",
            },
            functions: vec![(0x1020, 0x1220)],
            image_base: 0,
        };
        assert!(matches!(find(&img), Err(LocatorError::Missing { .. })));
    }
}
