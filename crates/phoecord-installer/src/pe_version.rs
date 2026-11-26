//! The file version of a Windows executable, read from its `VS_VERSIONINFO` resource.
//!
//! Discord Phoenix installs into a folder with no version in its name (`DiscordPhoenix\app`), so the
//! `app-x.y.z` trick that gives Stable, PTB and Canary their version does not work; the executable
//! itself says which build it is. The file is 500 MB, so this reads only the headers, the resource
//! directory and the version block, by seeking, and never maps or loads the image. Nothing runs.
//!
//! Any file that is not a well-formed PE with a version resource gives `None`; no input panics.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The resource type id of `VS_VERSIONINFO`.
const RT_VERSION: u32 = 16;
/// `VS_FIXEDFILEINFO.dwSignature`.
const FIXED_FILE_INFO: [u8; 4] = 0xFEEF_04BD_u32.to_le_bytes();
/// Bounds that keep a corrupt file from asking for a huge read.
const MAX_OPTIONAL_HEADER: usize = 1024;
const MAX_SECTIONS: usize = 96;
const MAX_VERSION_BLOCK: usize = 64 * 1024;
const MAX_DIR_ENTRIES: usize = 512;

/// `FileVersion` of the executable at `path` as `major.minor.build.revision` (`0.1.0.2208`), or
/// `None` when the file is missing, is not a PE image, or carries no version resource.
pub fn file_version(path: &Path) -> Option<String> {
    read_file_version(&mut File::open(path).ok()?)
}

/// [`file_version`] over any seekable source.
pub fn read_file_version<R: Read + Seek>(r: &mut R) -> Option<String> {
    // DOS header: "MZ", and at 0x3C the offset of the PE header.
    let dos = read_at(r, 0, 0x40)?;
    if dos[..2] != *b"MZ" {
        return None;
    }
    let pe = u64::from(u32_at(&dos, 0x3C)?);
    // "PE\0\0", the 20-byte COFF header, then the optional header.
    let head = read_at(r, pe, 24)?;
    if head[..4] != *b"PE\0\0" {
        return None;
    }
    let sections = usize::from(u16_at(&head, 6)?);
    let optional_len = usize::from(u16_at(&head, 20)?);
    if sections > MAX_SECTIONS || optional_len > MAX_OPTIONAL_HEADER {
        return None;
    }
    let optional = read_at(r, pe + 24, optional_len)?;
    // PE32 keeps its data directories at 96, PE32+ at 112; the resource table is the third one.
    let directories = match u16_at(&optional, 0)? {
        0x10B => 96,
        0x20B => 112,
        _ => return None,
    };
    if u32_at(&optional, directories - 4)? <= 2 {
        return None;
    }
    let resource_rva = u32_at(&optional, directories + 2 * 8)?;
    if resource_rva == 0 {
        return None;
    }
    let table = read_at(r, pe + 24 + optional_len as u64, sections * 40)?;
    let map = SectionMap::parse(&table)?;
    let root = map.offset(resource_rva)?;

    // Type -> name -> language: the first entry of the second and third level is enough, a
    // Discord executable has one version resource.
    let by_type = find_entry(r, root, root, Some(RT_VERSION))?;
    let by_name = find_entry(r, root, by_type, None)?;
    let leaf = find_entry(r, root, by_name, None)?;
    // `leaf` points at an IMAGE_RESOURCE_DATA_ENTRY: the RVA and the size of the version block.
    let data_entry = read_at(r, leaf, 8)?;
    let block_at = map.offset(u32_at(&data_entry, 0)?)?;
    let size = usize::try_from(u32_at(&data_entry, 4)?).ok()?;
    let block = read_at(r, block_at, size.min(MAX_VERSION_BLOCK))?;

    // The fixed part starts with its signature; the strings that follow are not needed.
    let at = block.windows(4).position(|w| w == FIXED_FILE_INFO)?;
    let ms = u32_at(&block, at + 8)?;
    let ls = u32_at(&block, at + 12)?;
    Some(format!(
        "{}.{}.{}.{}",
        ms >> 16,
        ms & 0xFFFF,
        ls >> 16,
        ls & 0xFFFF
    ))
}

/// Section headers: where each section sits in memory and in the file.
struct SectionMap(Vec<(u32, u32, u32)>);

impl SectionMap {
    fn parse(table: &[u8]) -> Option<SectionMap> {
        let mut out = Vec::new();
        for s in table.as_chunks::<40>().0.iter() {
            let virtual_size = u32_at(s, 8)?;
            let virtual_address = u32_at(s, 12)?;
            let raw_size = u32_at(s, 16)?;
            let raw_offset = u32_at(s, 20)?;
            out.push((virtual_address, virtual_size.max(raw_size), raw_offset));
        }
        Some(SectionMap(out))
    }

    /// The file offset of a relative virtual address.
    fn offset(&self, rva: u32) -> Option<u64> {
        self.0
            .iter()
            .find(|(va, len, _)| rva >= *va && rva - va < *len)
            .map(|(va, _, raw)| u64::from(*raw) + u64::from(rva - va))
    }
}

/// An entry of the resource directory at `dir` (a file offset): the one with id `want`, or the
/// first one when `want` is `None`. Returns the file offset of what it points at: a subdirectory
/// (relative to `root`) or a data entry.
fn find_entry<R: Read + Seek>(r: &mut R, root: u64, dir: u64, want: Option<u32>) -> Option<u64> {
    let head = read_at(r, dir, 16)?;
    let named = usize::from(u16_at(&head, 12)?);
    let ids = usize::from(u16_at(&head, 14)?);
    let total = named + ids;
    if total == 0 || total > MAX_DIR_ENTRIES {
        return None;
    }
    let entries = read_at(r, dir + 16, total * 8)?;
    for e in entries.as_chunks::<8>().0.iter() {
        let name = u32_at(e, 0)?;
        let target = u32_at(e, 4)?;
        let wanted = match want {
            Some(id) => name & 0x8000_0000 == 0 && name == id,
            None => true,
        };
        if wanted {
            // The high bit says "a subdirectory"; the rest is an offset from the directory root.
            return Some(root + u64::from(target & 0x7FFF_FFFF));
        }
    }
    None
}

fn read_at<R: Read + Seek>(r: &mut R, at: u64, len: usize) -> Option<Vec<u8>> {
    r.seek(SeekFrom::Start(at)).ok()?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A minimal PE image with one `.rsrc` section holding a version resource; the tests build their
/// "Discord.exe" files with it.
#[cfg(test)]
pub(crate) fn fake_pe(version: Option<(u16, u16, u16, u16)>) -> Vec<u8> {
    fn put(b: &mut Vec<u8>, at: usize, bytes: &[u8]) {
        if b.len() < at + bytes.len() {
            b.resize(at + bytes.len(), 0);
        }
        b[at..at + bytes.len()].copy_from_slice(bytes);
    }
    let mut b = Vec::new();
    put(&mut b, 0, b"MZ");
    put(&mut b, 0x3C, &0x80u32.to_le_bytes());
    put(&mut b, 0x80, b"PE\0\0");
    put(&mut b, 0x84 + 2, &1u16.to_le_bytes()); // one section
    put(&mut b, 0x84 + 16, &240u16.to_le_bytes()); // PE32+ optional header size
    let opt = 0x98;
    put(&mut b, opt, &0x20Bu16.to_le_bytes());
    put(&mut b, opt + 108, &16u32.to_le_bytes()); // data directory count
    let rsrc_rva = 0x3000u32;
    put(&mut b, opt + 112 + 2 * 8, &rsrc_rva.to_le_bytes());
    let section = opt + 240;
    let rsrc_raw = 0x400u32;
    put(&mut b, section, b".rsrc\0\0\0");
    put(&mut b, section + 8, &0x200u32.to_le_bytes()); // virtual size
    put(&mut b, section + 12, &rsrc_rva.to_le_bytes());
    put(&mut b, section + 16, &0x200u32.to_le_bytes()); // raw size
    put(&mut b, section + 20, &rsrc_raw.to_le_bytes());
    b.resize(rsrc_raw as usize, 0);
    if let Some((a, c, d, e)) = version {
        let r = rsrc_raw as usize;
        // root directory: one id entry (type 16) -> directory at 0x18
        put(&mut b, r + 14, &1u16.to_le_bytes());
        put(&mut b, r + 16, &RT_VERSION.to_le_bytes());
        put(&mut b, r + 20, &(0x8000_0000u32 | 0x18).to_le_bytes());
        // name directory at 0x18: one id entry (1) -> directory at 0x30
        put(&mut b, r + 0x18 + 14, &1u16.to_le_bytes());
        put(&mut b, r + 0x18 + 16, &1u32.to_le_bytes());
        put(
            &mut b,
            r + 0x18 + 20,
            &(0x8000_0000u32 | 0x30).to_le_bytes(),
        );
        // language directory at 0x30: one id entry (0x409) -> data entry at 0x48
        put(&mut b, r + 0x30 + 14, &1u16.to_le_bytes());
        put(&mut b, r + 0x30 + 16, &0x409u32.to_le_bytes());
        put(&mut b, r + 0x30 + 20, &0x48u32.to_le_bytes());
        // data entry: RVA and size of the version block at 0x60
        put(&mut b, r + 0x48, &(rsrc_rva + 0x60).to_le_bytes());
        put(&mut b, r + 0x48 + 4, &0x60u32.to_le_bytes());
        // VS_VERSIONINFO header (length, value length, type, "VS_VERSION_INFO"), padded to the
        // fixed part, which starts with its signature.
        let block = r + 0x60;
        put(&mut b, block, &0x5Cu16.to_le_bytes());
        put(&mut b, block + 2, &0x34u16.to_le_bytes());
        let key: Vec<u8> = "VS_VERSION_INFO"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        put(&mut b, block + 6, &key);
        let fixed = block + 0x28;
        put(&mut b, fixed, &FIXED_FILE_INFO);
        put(&mut b, fixed + 4, &0x0001_0000u32.to_le_bytes());
        let ms = (u32::from(a) << 16) | u32::from(c);
        let ls = (u32::from(d) << 16) | u32::from(e);
        put(&mut b, fixed + 8, &ms.to_le_bytes()); // file version
        put(&mut b, fixed + 12, &ls.to_le_bytes());
        put(&mut b, fixed + 16, &ms.to_le_bytes()); // product version
        put(&mut b, fixed + 20, &ls.to_le_bytes());
    }
    b.resize(rsrc_raw as usize + 0x200, 0);
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_the_file_version_of_a_pe() {
        let pe = fake_pe(Some((0, 1, 0, 2208)));
        assert_eq!(
            read_file_version(&mut Cursor::new(pe)).as_deref(),
            Some("0.1.0.2208")
        );
        let pe = fake_pe(Some((1, 0, 9270, 3)));
        assert_eq!(
            read_file_version(&mut Cursor::new(pe)).as_deref(),
            Some("1.0.9270.3")
        );
    }

    #[test]
    fn a_pe_without_a_version_resource_has_no_version() {
        assert_eq!(read_file_version(&mut Cursor::new(fake_pe(None))), None);
    }

    #[test]
    fn anything_that_is_not_a_pe_has_no_version() {
        for bytes in [
            Vec::new(),
            b"MZ".to_vec(),
            b"#!/bin/sh\necho hi\n".to_vec(),
            vec![0xFF; 4096],
        ] {
            assert_eq!(read_file_version(&mut Cursor::new(bytes)), None);
        }
    }

    #[test]
    fn a_truncated_or_corrupt_pe_never_panics() {
        let good = fake_pe(Some((0, 1, 0, 2208)));
        // Cut at every length: some prefixes still hold the version, none may panic.
        for len in 0..good.len() {
            let _ = read_file_version(&mut Cursor::new(good[..len].to_vec()));
        }
        // A directory entry count far beyond the file, and a section table that overflows.
        let mut bad = good.clone();
        bad[0x400 + 14] = 0xFF;
        bad[0x400 + 15] = 0xFF;
        assert_eq!(read_file_version(&mut Cursor::new(bad)), None);
        let mut bad = good;
        bad[0x84 + 2] = 0xFF; // 255 sections
        assert_eq!(read_file_version(&mut Cursor::new(bad)), None);
    }

    #[test]
    fn a_path_that_does_not_exist_has_no_version() {
        assert_eq!(
            file_version(Path::new("/definitely/not/here/Discord.exe")),
            None
        );
    }
}
