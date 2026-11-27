//! Locator for `DiscordApp::render`'s entry (task 2.3.21: PhoeCord's own overlay).
//!
//! `docs/re/gpui-lifecycle.md` §4 names the global UI injection point: "hook the exit of
//! `DiscordApp::render` (`0x144a44984`). Once the root `Div` is built..., call `Div::child`
//! (`0x14842bc60`) to add a custom `AnyElement` / `FnView` ... on top of the app tree." That
//! note's own addresses turn out to match this exact installed build (`0.1.0.2208`, same hash as
//! `phoenix-recon.md`'s Part 1 — confirmed independently while chasing 2.3.4/2.3.10's anchors,
//! `phoecord-recon`, 2026-11-27): `DiscordApp::render`, `Div::child` and the `FnView` vtable it
//! names all resolve to real, `.pdata`-bounded functions (or, for the vtable, the same
//! `drop`/`size`/`align`/`fn`/`fn` shape already confirmed for the HTTP transport and `RowView`)
//! in this build.
//!
//! This locates `DiscordApp::render`'s entry, the hook point itself. `Div::child` and the
//! `FnView` vtable shape are facts to build a custom overlay element with once hooked here, not
//! separate hook targets — the project's locators are for things to intercept, and nobody
//! intercepts `Div::child` or a vtable layout.
//!
//! Checked against Phoenix `0.1.0.2208` only; not yet verified on a second build (task 2.2.9).

use super::{Found, Image, LocatorError, exactly_one};
use crate::aob::Pattern;

/// Prologue of `DiscordApp::render`, RVA `0x4a44984` in the checked build. Matches exactly once
/// in `.text` — no cross-reference needed.
const RENDER_AOB: &str = "55 41 57 41 56 41 55 41 54 56 57 53 48 81 EC B8 0A 00 00 \
     48 8D AC 24 80 00 00 00 48 C7 85 30 0A 00 00 FE FF FF FF \
     4D 89 CE 4C 89 C7 49 89 D5 48 89 CB 49 8B 31 48 89 F1";

/// `DiscordApp::render`'s entry, the whole app tree's root render function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscordAppRender {
    /// Entry RVA, for an inline hook on its exit.
    pub rva: u32,
}

/// Locates `DiscordApp::render`.
pub fn find(image: &Image<'_>) -> Result<Found<DiscordAppRender>, LocatorError> {
    let pattern = Pattern::parse(RENDER_AOB).expect("locator AOB is a fixed, valid pattern");
    let matches = pattern
        .find_all(image.text.bytes)
        .map(|off| image.text.rva + off as u32);
    let rva = exactly_one("DiscordApp::render", matches)?;
    Ok(Found::new(
        DiscordAppRender { rva },
        "masked AOB on the function's own prologue, unique alone \
         (docs/re/gpui-lifecycle.md, confirmed against this build, phoecord-recon 2026-11-27)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locators::Region;

    fn pattern_bytes() -> Vec<u8> {
        RENDER_AOB
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
    fn finds_discord_app_render() {
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
                what: "DiscordApp::render".into()
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
