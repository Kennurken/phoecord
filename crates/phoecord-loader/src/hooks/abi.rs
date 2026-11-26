//! Reading the client's REST request out of its memory (task 2.2.2, part 1).
//!
//! The `send` detour gets two pointers: the transport (`self`) and the request. This module turns
//! the request pointer into a method, a URL and a body, through a [`Memory`] that may refuse any
//! read. Nothing here dereferences a raw pointer: the DLL reads through `ReadProcessMemory` on its
//! own process ([`super::detour`]), so a wrong offset costs one refused read, not a crash, and the
//! tests read from plain byte buffers on every OS.
//!
//! The layout is the one `send` reads in Phoenix `0.1.0.2208` (private recon notes, 2.1.5 second
//! pass). Every field is checked before it is trusted; a request that does not look right is
//! reported as [`AbiError`] and the detour lets it through untouched.
//!
//! | Offset | Field | Status in the recon notes |
//! |:---|:---|:---|
//! | `+0x60` | body tag: `0` none, `1` `bytes::Bytes`, others not decoded | tags 0 and 1 verified |
//! | `+0x68` | `bytes::Bytes`: four words | verified; word order inferred (see [`BYTES_PTR`]) |
//! | `+0x98` | URL `String`: capacity, pointer, length | verified |
//! | `+0xc0` | `http::Method`, 24 bytes | inferred (see [`decode_method`]) |

use std::fmt;
use std::ops::Range;

/// Offset of the body tag in the request.
pub const BODY_TAG: usize = 0x60;
/// Offset of the `bytes::Bytes` of a tag-1 body.
pub const BODY_BYTES: usize = 0x68;
/// Offset of the URL `String` (capacity, pointer, length).
pub const URL: usize = 0x98;
/// Offset of the `http::Method`.
pub const METHOD: usize = 0xc0;
/// Size of `http::Method` in this build.
pub const METHOD_LEN: usize = 0x18;

/// Word index of the data pointer inside `Bytes`. `bytes` 1.x declares `ptr, len, data, vtable`,
/// all pointer-sized, which rustc keeps in that order; the recon notes leave the order unverified,
/// so [`read_body`] checks that the vtable word points into the client image before it trusts the
/// other three.
pub const BYTES_PTR: usize = 0;
pub const BYTES_LEN: usize = 1;
pub const BYTES_VTABLE: usize = 3;

/// The longest URL read; anything longer is refused as a wrong reading.
pub const MAX_URL: usize = 64 * 1024;
/// The largest body read for plugins; a bigger one passes untouched (an upload).
pub const MAX_BODY: usize = 8 * 1024 * 1024;

/// Reads of the client's memory that may fail instead of faulting.
pub trait Memory {
    /// Fills `buf` from `addr`; `false` when any byte of the range is not readable.
    fn read(&self, addr: usize, buf: &mut [u8]) -> bool;

    fn word(&self, addr: usize) -> Option<usize> {
        let mut b = [0u8; 8];
        self.read(addr, &mut b)
            .then(|| u64::from_le_bytes(b) as usize)
    }
}

/// Why a request was not read; the detour logs it once per kind and passes the call through.
#[derive(Debug, PartialEq, Eq)]
pub enum AbiError {
    /// A read at this address was refused.
    Unreadable(usize),
    /// The URL `String` does not look like one (null pointer, length over capacity, too long).
    BadUrl,
    /// The URL bytes are not an `http(s)` URL.
    NotHttp,
    /// The method word is not one of the encodings this build is known to use.
    UnknownMethod(u8),
}

impl fmt::Display for AbiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AbiError::Unreadable(a) => write!(f, "cannot read client memory at {a:#x}"),
            AbiError::BadUrl => f.write_str("the URL field does not hold a string"),
            AbiError::NotHttp => f.write_str("the URL field does not hold an http(s) URL"),
            AbiError::UnknownMethod(t) => write!(f, "unknown method encoding (tag {t})"),
        }
    }
}

impl std::error::Error for AbiError {}

/// The parts of a request that decide whether plugins want it.
#[derive(Debug, PartialEq, Eq)]
pub struct RequestHead {
    pub method: String,
    pub url: String,
}

/// The body as plugins can see it.
#[derive(Debug, PartialEq, Eq)]
pub enum Body {
    None,
    Bytes(Vec<u8>),
    /// A body this module cannot read safely (another tag, a stream, too large, a layout that
    /// does not check out). Plugins see no body, and a changed body cannot be handed back.
    Opaque,
}

/// The standard methods in `http::Method`'s declaration order, which is its tag.
const STANDARD: [&str; 9] = [
    "OPTIONS", "GET", "POST", "PUT", "DELETE", "HEAD", "TRACE", "CONNECT", "PATCH",
];
/// Tag of `ExtensionInline`: up to 15 name bytes from offset 1, their count at offset 16.
const TAG_INLINE: u8 = 9;
/// Tag of `ExtensionAllocated`: a `Box<[u8]>` (pointer, length) at offset 8.
const TAG_ALLOCATED: u8 = 10;

/// `http::Method` from its 24 bytes. `http` 1.x stores it as a tagged enum: nine standard
/// variants, then an inline and an allocated extension. Discord's REST client only sends
/// standard methods, so an extension is decoded only when it reads as an ASCII token.
pub fn decode_method(mem: &dyn Memory, raw: &[u8; METHOD_LEN]) -> Result<String, AbiError> {
    let tag = raw[0];
    if let Some(m) = STANDARD.get(usize::from(tag)) {
        return Ok((*m).to_string());
    }
    let bytes = match tag {
        TAG_INLINE => {
            let n = usize::from(raw[16]);
            raw.get(1..1 + n)
                .ok_or(AbiError::UnknownMethod(tag))?
                .to_vec()
        }
        TAG_ALLOCATED => {
            let ptr = word_at(raw, 8);
            let len = word_at(raw, 16);
            if ptr == 0 || len == 0 || len > 32 {
                return Err(AbiError::UnknownMethod(tag));
            }
            let mut b = vec![0u8; len];
            if !mem.read(ptr, &mut b) {
                return Err(AbiError::Unreadable(ptr));
            }
            b
        }
        _ => return Err(AbiError::UnknownMethod(tag)),
    };
    if bytes.is_empty() || !bytes.iter().all(|b| b.is_ascii_alphabetic() || *b == b'-') {
        return Err(AbiError::UnknownMethod(tag));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn word_at(raw: &[u8], at: usize) -> usize {
    let mut b = [0u8; 8];
    b.copy_from_slice(&raw[at..at + 8]);
    u64::from_le_bytes(b) as usize
}

/// Method and URL of the request at `req`.
pub fn read_head(mem: &dyn Memory, req: usize) -> Result<RequestHead, AbiError> {
    let word = |off: usize| {
        let at = req + off;
        mem.word(at).ok_or(AbiError::Unreadable(at))
    };
    let (cap, ptr, len) = (word(URL)?, word(URL + 8)?, word(URL + 16)?);
    if ptr == 0 || len == 0 || len > cap || len > MAX_URL {
        return Err(AbiError::BadUrl);
    }
    let mut url = vec![0u8; len];
    if !mem.read(ptr, &mut url) {
        return Err(AbiError::Unreadable(ptr));
    }
    let url = String::from_utf8(url).map_err(|_| AbiError::NotHttp)?;
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(AbiError::NotHttp);
    }
    let mut raw = [0u8; METHOD_LEN];
    if !mem.read(req + METHOD, &mut raw) {
        return Err(AbiError::Unreadable(req + METHOD));
    }
    Ok(RequestHead {
        method: decode_method(mem, &raw)?,
        url,
    })
}

/// The body of the request at `req`. `image` is the client module's address range: the `Bytes`
/// vtable is a static of the client, so a vtable word outside it means the reading is wrong.
pub fn read_body(mem: &dyn Memory, req: usize, image: Range<usize>) -> Body {
    let Some(tag) = mem.word(req + BODY_TAG) else {
        return Body::Opaque;
    };
    match tag {
        0 => Body::None,
        1 => {
            let w = |i: usize| mem.word(req + BODY_BYTES + 8 * i);
            let (Some(ptr), Some(len), Some(vtable)) =
                (w(BYTES_PTR), w(BYTES_LEN), w(BYTES_VTABLE))
            else {
                return Body::Opaque;
            };
            if !image.contains(&vtable) || len > MAX_BODY {
                return Body::Opaque;
            }
            if len == 0 {
                return Body::Bytes(Vec::new());
            }
            if ptr == 0 {
                return Body::Opaque;
            }
            let mut b = vec![0u8; len];
            if mem.read(ptr, &mut b) {
                Body::Bytes(b)
            } else {
                Body::Opaque
            }
        }
        _ => Body::Opaque,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Separate regions of fake memory; a read must fall inside one of them.
    struct Fake(Vec<(usize, Vec<u8>)>);

    impl Memory for Fake {
        fn read(&self, addr: usize, buf: &mut [u8]) -> bool {
            self.0.iter().any(|(base, bytes)| {
                let Some(off) = addr.checked_sub(*base) else {
                    return false;
                };
                match bytes.get(off..off + buf.len()) {
                    Some(src) => {
                        buf.copy_from_slice(src);
                        true
                    }
                    None => false,
                }
            })
        }
    }

    const REQ: usize = 0x1000;
    const URL_AT: usize = 0x5000;
    const BODY_AT: usize = 0x6000;
    const IMAGE: Range<usize> = 0x10_0000..0x20_0000;
    const VTABLE: usize = 0x15_0000;

    fn put(buf: &mut [u8], at: usize, w: usize) {
        buf[at..at + 8].copy_from_slice(&(w as u64).to_le_bytes());
    }

    /// Overwrites one word of the fake request.
    fn patch(mem: &mut Fake, at: usize, w: usize) {
        let req = &mut mem.0.iter_mut().find(|(b, _)| *b == REQ).unwrap().1;
        put(req, at, w);
    }

    fn request(url: &str, method_tag: u8, body: Option<&[u8]>) -> Fake {
        let mut req = vec![0u8; 0xf0];
        put(&mut req, URL, url.len() + 3);
        put(&mut req, URL + 8, URL_AT);
        put(&mut req, URL + 16, url.len());
        req[METHOD] = method_tag;
        let mut regions = vec![(URL_AT, url.as_bytes().to_vec())];
        if let Some(b) = body {
            put(&mut req, BODY_TAG, 1);
            put(&mut req, BODY_BYTES, BODY_AT);
            put(&mut req, BODY_BYTES + 8, b.len());
            put(&mut req, BODY_BYTES + 16, BODY_AT);
            put(&mut req, BODY_BYTES + 24, VTABLE);
            regions.push((BODY_AT, b.to_vec()));
        }
        regions.push((REQ, req));
        Fake(regions)
    }

    #[test]
    fn reads_method_url_and_body() {
        let mem = request("https://discord.com/api/v9/science", 2, Some(br#"{"a":1}"#));
        assert_eq!(
            read_head(&mem, REQ).unwrap(),
            RequestHead {
                method: "POST".into(),
                url: "https://discord.com/api/v9/science".into()
            }
        );
        assert_eq!(
            read_body(&mem, REQ, IMAGE),
            Body::Bytes(br#"{"a":1}"#.to_vec())
        );
    }

    #[test]
    fn a_request_without_body_has_none() {
        let mem = request("https://discord.com/api/v9/users/@me", 1, None);
        assert_eq!(read_head(&mem, REQ).unwrap().method, "GET");
        assert_eq!(read_body(&mem, REQ, IMAGE), Body::None);
    }

    #[test]
    fn a_vtable_outside_the_image_makes_the_body_opaque() {
        let mut mem = request("https://discord.com/api/v9/x", 2, Some(b"{}"));
        patch(&mut mem, BODY_BYTES + 24, 0x42);
        assert_eq!(read_body(&mem, REQ, IMAGE), Body::Opaque);
    }

    #[test]
    fn unknown_body_tags_and_huge_bodies_are_opaque() {
        let mut mem = request("https://discord.com/api/v9/x", 2, Some(b"{}"));
        patch(&mut mem, BODY_TAG, 2);
        assert_eq!(read_body(&mem, REQ, IMAGE), Body::Opaque);
        patch(&mut mem, BODY_TAG, 1);
        patch(&mut mem, BODY_BYTES + 8, MAX_BODY + 1);
        assert_eq!(read_body(&mem, REQ, IMAGE), Body::Opaque);
    }

    #[test]
    fn a_url_field_that_is_not_a_string_is_refused() {
        let mut mem = request("https://discord.com/api/v9/x", 1, None);
        // Length over capacity.
        patch(&mut mem, URL, 2);
        assert_eq!(read_head(&mem, REQ), Err(AbiError::BadUrl));
        // Null pointer.
        patch(&mut mem, URL, 100);
        patch(&mut mem, URL + 8, 0);
        assert_eq!(read_head(&mem, REQ), Err(AbiError::BadUrl));
        // A pointer to unreadable memory.
        patch(&mut mem, URL + 8, 0xdead_0000);
        assert_eq!(read_head(&mem, REQ), Err(AbiError::Unreadable(0xdead_0000)));
    }

    #[test]
    fn a_url_that_is_not_http_is_refused() {
        let mem = request("C:\\Windows\\win.ini", 1, None);
        assert_eq!(read_head(&mem, REQ), Err(AbiError::NotHttp));
    }

    #[test]
    fn an_unreadable_request_is_reported() {
        let mem = Fake(Vec::new());
        assert_eq!(read_head(&mem, REQ), Err(AbiError::Unreadable(REQ + URL)));
        assert_eq!(read_body(&mem, REQ, IMAGE), Body::Opaque);
    }

    #[test]
    fn methods_decode_by_tag_and_extensions_must_be_tokens() {
        let mem = Fake(vec![(0x9000, b"PROPFIND".to_vec())]);
        let mut raw = [0u8; METHOD_LEN];
        for (tag, name) in STANDARD.iter().enumerate() {
            raw[0] = tag as u8;
            assert_eq!(decode_method(&mem, &raw).unwrap(), *name);
        }
        raw[0] = TAG_INLINE;
        raw[1..6].copy_from_slice(b"PURGE");
        raw[16] = 5;
        assert_eq!(decode_method(&mem, &raw).unwrap(), "PURGE");
        raw[16] = 40;
        assert_eq!(
            decode_method(&mem, &raw),
            Err(AbiError::UnknownMethod(TAG_INLINE))
        );
        let mut raw = [0u8; METHOD_LEN];
        raw[0] = TAG_ALLOCATED;
        put(&mut raw, 8, 0x9000);
        put(&mut raw, 16, 8);
        assert_eq!(decode_method(&mem, &raw).unwrap(), "PROPFIND");
        put(&mut raw, 8, 0x7000);
        assert_eq!(decode_method(&mem, &raw), Err(AbiError::Unreadable(0x7000)));
        raw[0] = 11;
        assert_eq!(decode_method(&mem, &raw), Err(AbiError::UnknownMethod(11)));
        let mut raw = [0u8; METHOD_LEN];
        raw[0] = TAG_INLINE;
        raw[1] = 0xff;
        raw[16] = 1;
        assert_eq!(
            decode_method(&mem, &raw),
            Err(AbiError::UnknownMethod(TAG_INLINE))
        );
    }
}
