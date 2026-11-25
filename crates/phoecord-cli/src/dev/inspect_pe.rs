//! `phoecord dev inspect-pe <exe>` (task 2.1.2, binary inspection).
//!
//! A read-only look at a Windows PE image: which DLLs it imports (and which of them are candidates
//! for sideloading), the DLL search restrictions recorded in the image (`DllCharacteristics`, the
//! `DependentLoadFlags` of the Load Configuration directory), TLS callbacks, and the section layout
//! that an AOB scan works against. The file is only read; nothing is loaded or executed.

use crate::tf;
use anyhow::{Context, Result, bail};
use clap::Args;
use object::LittleEndian as LE;
use object::pe;
use object::pe::{ImageNtHeaders32, ImageNtHeaders64};
use object::read::pe::{ImageNtHeaders, ImageOptionalHeader, PeFile};
use phoecord_i18n::Localizer;
use std::path::PathBuf;

/// DLLs worth a note when a client imports them: the usual sideload and proxy-DLL carriers.
const CANDIDATES: &[&str] = &[
    "version.dll",
    "winhttp.dll",
    "winmm.dll",
    "wininet.dll",
    "dwmapi.dll",
    "dbghelp.dll",
];

/// Most TLS callbacks listed; a real image has a handful.
const MAX_TLS_CALLBACKS: usize = 64;

/// `DllCharacteristics` bits, by name.
const DLL_CHARACTERISTICS: &[(u16, &str)] = &[
    (
        pe::IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA,
        "HIGH_ENTROPY_VA",
    ),
    (pe::IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE, "DYNAMIC_BASE"),
    (
        pe::IMAGE_DLLCHARACTERISTICS_FORCE_INTEGRITY,
        "FORCE_INTEGRITY",
    ),
    (pe::IMAGE_DLLCHARACTERISTICS_NX_COMPAT, "NX_COMPAT"),
    (pe::IMAGE_DLLCHARACTERISTICS_NO_ISOLATION, "NO_ISOLATION"),
    (pe::IMAGE_DLLCHARACTERISTICS_NO_SEH, "NO_SEH"),
    (pe::IMAGE_DLLCHARACTERISTICS_NO_BIND, "NO_BIND"),
    (pe::IMAGE_DLLCHARACTERISTICS_APPCONTAINER, "APPCONTAINER"),
    (pe::IMAGE_DLLCHARACTERISTICS_WDM_DRIVER, "WDM_DRIVER"),
    (pe::IMAGE_DLLCHARACTERISTICS_GUARD_CF, "GUARD_CF"),
    (
        pe::IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE,
        "TERMINAL_SERVER_AWARE",
    ),
];

/// `LOAD_LIBRARY_SEARCH_*` bits as stored in `DependentLoadFlags`.
const DEPENDENT_LOAD_FLAGS: &[(u16, &str)] = &[
    (0x0100, "SEARCH_DLL_LOAD_DIR"),
    (0x0200, "SEARCH_APPLICATION_DIR"),
    (0x0400, "SEARCH_USER_DIRS"),
    (0x0800, "SEARCH_SYSTEM32"),
    (0x1000, "SEARCH_DEFAULT_DIRS"),
    (0x2000, "SAFE_CURRENT_DIRS"),
];

#[derive(Args)]
pub struct InspectPeArgs {
    /// The PE executable to inspect (for example Discord.exe).
    file: PathBuf,
    /// List the imported function names of every DLL, not only the counts.
    #[arg(long)]
    functions: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct ImportedDll {
    name: String,
    functions: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct SectionInfo {
    name: String,
    virtual_address: u32,
    virtual_size: u32,
    raw_offset: u32,
    raw_size: u32,
    characteristics: u32,
}

#[derive(Debug, PartialEq, Eq)]
struct LoadConfig {
    size: u32,
    /// `None` when the structure is too short to hold the field.
    dependent_load_flags: Option<u16>,
}

#[derive(Debug)]
struct Report {
    is_64: bool,
    machine: u16,
    subsystem: u16,
    image_base: u64,
    entry_rva: u32,
    dll_characteristics: u16,
    imports: Vec<ImportedDll>,
    delay_imports: Vec<ImportedDll>,
    load_config: Option<LoadConfig>,
    tls_callbacks: Vec<u64>,
    has_tls: bool,
    has_clr: bool,
    sections: Vec<SectionInfo>,
}

pub fn run(args: InspectPeArgs, l10n: &Localizer) -> Result<()> {
    let shown = args.file.display().to_string();
    let data = std::fs::read(&args.file)
        .with_context(|| tf(l10n, "dev-pe-err-read", &[("path", &shown)]))?;
    let report = parse(&data).with_context(|| tf(l10n, "dev-pe-err-parse", &[("path", &shown)]))?;
    print!("{}", render(&report, args.functions, l10n));
    Ok(())
}

fn parse(data: &[u8]) -> Result<Report> {
    match object::read::pe::optional_header_magic(data)? {
        pe::IMAGE_NT_OPTIONAL_HDR32_MAGIC => inspect::<ImageNtHeaders32>(data),
        pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC => inspect::<ImageNtHeaders64>(data),
        _ => bail!("unknown optional header magic"),
    }
}

fn inspect<Pe: ImageNtHeaders>(data: &[u8]) -> Result<Report> {
    let file = PeFile::<Pe, &[u8]>::parse(data)?;
    let nt = file.nt_headers();
    let opt = nt.optional_header();
    let sections = file.section_table();
    let dirs = file.data_directories();
    let image_base = opt.image_base();

    let mut imports = Vec::new();
    if let Some(table) = dirs.import_table(data, &sections)? {
        let mut it = table.descriptors()?;
        while let Some(d) = it.next()? {
            let name = dll_name(table.name(d.name.get(LE))?);
            let first = match d.original_first_thunk.get(LE) {
                0 => d.first_thunk.get(LE),
                rva => rva,
            };
            let mut functions = Vec::new();
            let mut thunks = table.thunks(first)?;
            while let Some(t) = thunks.next::<Pe>()? {
                functions.push(function_name(table.import::<Pe>(t)?));
            }
            imports.push(ImportedDll { name, functions });
        }
    }

    let mut delay_imports = Vec::new();
    if let Some(table) = dirs.delay_load_import_table(data, &sections)? {
        let mut it = table.descriptors()?;
        while let Some(d) = it.next()? {
            let name = dll_name(table.name(d.dll_name_rva.get(LE))?);
            let mut functions = Vec::new();
            let mut thunks = table.thunks(d.import_name_table_rva.get(LE))?;
            while let Some(t) = thunks.next::<Pe>()? {
                functions.push(function_name(table.import::<Pe>(t)?));
            }
            delay_imports.push(ImportedDll { name, functions });
        }
    }

    let load_config = match dirs.get(pe::IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG) {
        Some(dir) if dir.virtual_address.get(LE) != 0 => {
            let bytes = dir.data(data, &sections)?;
            Some(load_config(bytes, Pe::is_type_64(nt))?)
        }
        _ => None,
    };

    let tls_dir = dirs.get(pe::IMAGE_DIRECTORY_ENTRY_TLS);
    let has_tls = tls_dir.is_some_and(|d| d.virtual_address.get(LE) != 0);
    let mut tls_callbacks = Vec::new();
    if let Some(dir) = tls_dir.filter(|_| has_tls) {
        let bytes = dir.data(data, &sections)?;
        tls_callbacks = tls_callbacks_of(data, &sections, bytes, image_base, Pe::is_type_64(nt));
    }

    let has_clr = dirs
        .get(pe::IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR)
        .is_some_and(|d| d.virtual_address.get(LE) != 0);

    let sections_out = sections
        .iter()
        .map(|s| SectionInfo {
            name: section_name(&s.name),
            virtual_address: s.virtual_address.get(LE),
            virtual_size: s.virtual_size.get(LE),
            raw_offset: s.pointer_to_raw_data.get(LE),
            raw_size: s.size_of_raw_data.get(LE),
            characteristics: s.characteristics.get(LE),
        })
        .collect();

    Ok(Report {
        is_64: Pe::is_type_64(nt),
        machine: nt.file_header().machine.get(LE),
        subsystem: opt.subsystem(),
        image_base,
        entry_rva: opt.address_of_entry_point(),
        dll_characteristics: opt.dll_characteristics(),
        imports,
        delay_imports,
        load_config,
        tls_callbacks,
        has_tls,
        has_clr,
        sections: sections_out,
    })
}

/// Reads `Size` and `DependentLoadFlags` from the raw Load Configuration bytes. The structure grew
/// over Windows versions, so the field is read only when `Size` says it is there.
fn load_config(bytes: &[u8], is_64: bool) -> Result<LoadConfig> {
    // Offsets of `DependentLoadFlags`: after `CSDVersion`, which follows `ProcessHeapFlags`.
    let flags_at = if is_64 { 0x50 } else { 0x38 };
    let Some(size) = read_u32(bytes, 0) else {
        bail!("the Load Configuration directory is truncated");
    };
    let dependent_load_flags = if size as usize >= flags_at + 2 {
        read_u16(bytes, flags_at)
    } else {
        None
    };
    Ok(LoadConfig {
        size,
        dependent_load_flags,
    })
}

/// The TLS callback array is a null-terminated list of virtual addresses; returns them as RVAs.
fn tls_callbacks_of(
    data: &[u8],
    sections: &object::read::pe::SectionTable<'_>,
    dir: &[u8],
    image_base: u64,
    is_64: bool,
) -> Vec<u64> {
    let (at, width) = if is_64 { (24, 8) } else { (12, 4) };
    let va = if is_64 {
        read_u64(dir, at)
    } else {
        read_u32(dir, at).map(u64::from)
    };
    let Some(rva) = va
        .and_then(|v| v.checked_sub(image_base))
        .and_then(|r| u32::try_from(r).ok())
        .filter(|r| *r != 0)
    else {
        return Vec::new();
    };
    let Some(array) = sections.pe_data_at(data, rva) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for i in 0..MAX_TLS_CALLBACKS {
        let entry = if width == 8 {
            read_u64(array, i * 8)
        } else {
            read_u32(array, i * 4).map(u64::from)
        };
        match entry {
            Some(v) if v != 0 => out.push(v.saturating_sub(image_base)),
            _ => break,
        }
    }
    out
}

fn read_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn read_u64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn dll_name(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).into_owned()
}

fn function_name(import: object::read::pe::Import<'_>) -> String {
    match import {
        object::read::pe::Import::Ordinal(n) => format!("#{n}"),
        object::read::pe::Import::Name(_, name) => String::from_utf8_lossy(name).into_owned(),
    }
}

fn section_name(raw: &[u8; 8]) -> String {
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

fn is_candidate(name: &str) -> bool {
    CANDIDATES.iter().any(|c| c.eq_ignore_ascii_case(name))
}

fn flag_names(value: u32, table: &[(u16, &str)]) -> String {
    let names: Vec<&str> = table
        .iter()
        .filter(|(bit, _)| value & u32::from(*bit) != 0)
        .map(|(_, n)| *n)
        .collect();
    if names.is_empty() {
        "-".to_string()
    } else {
        names.join(" | ")
    }
}

fn machine_name(machine: u16) -> String {
    match machine {
        pe::IMAGE_FILE_MACHINE_AMD64 => "x86-64".to_string(),
        pe::IMAGE_FILE_MACHINE_I386 => "x86".to_string(),
        pe::IMAGE_FILE_MACHINE_ARM64 => "ARM64".to_string(),
        other => format!("machine {other:#06x}"),
    }
}

fn subsystem_name(subsystem: u16) -> String {
    match subsystem {
        pe::IMAGE_SUBSYSTEM_WINDOWS_GUI => "Windows GUI".to_string(),
        pe::IMAGE_SUBSYSTEM_WINDOWS_CUI => "Windows console".to_string(),
        other => format!("subsystem {other}"),
    }
}

/// `r-x`-style summary of the section's memory flags.
fn section_flags(c: u32) -> String {
    let bit = |mask: u32, ch: char| if c & mask != 0 { ch } else { '-' };
    [
        bit(pe::IMAGE_SCN_MEM_READ, 'r'),
        bit(pe::IMAGE_SCN_MEM_WRITE, 'w'),
        bit(pe::IMAGE_SCN_MEM_EXECUTE, 'x'),
    ]
    .iter()
    .collect()
}

fn render(r: &Report, functions: bool, l10n: &Localizer) -> String {
    let mut out = String::new();
    let mut line = |s: String| {
        out.push_str(&s);
        out.push('\n');
    };
    let hex = |v: u64| format!("{v:#x}");

    line(tf(
        l10n,
        "dev-pe-header",
        &[
            ("machine", &machine_name(r.machine)),
            ("bits", if r.is_64 { "PE32+" } else { "PE32" }),
            ("subsystem", &subsystem_name(r.subsystem)),
            ("entry", &hex(r.entry_rva.into())),
            ("base", &hex(r.image_base)),
        ],
    ));
    let yes_no = |b: bool| l10n.t(if b { "dev-pe-yes" } else { "dev-pe-no" });
    line(tf(l10n, "dev-pe-clr", &[("value", &yes_no(r.has_clr))]));

    for (key, dlls) in [
        ("dev-pe-imports", &r.imports),
        ("dev-pe-delay-imports", &r.delay_imports),
    ] {
        line(String::new());
        line(tf(l10n, key, &[("count", &dlls.len().to_string())]));
        for d in dlls {
            let mark = if is_candidate(&d.name) { "*" } else { " " };
            line(tf(
                l10n,
                "dev-pe-import-line",
                &[
                    ("mark", mark),
                    ("name", &d.name),
                    ("count", &d.functions.len().to_string()),
                ],
            ));
            if functions {
                for f in &d.functions {
                    line(format!("      {f}"));
                }
            }
        }
    }
    let found: Vec<&str> = r
        .imports
        .iter()
        .chain(&r.delay_imports)
        .map(|d| d.name.as_str())
        .filter(|n| is_candidate(n))
        .collect();
    let list = if found.is_empty() {
        CANDIDATES.join(", ")
    } else {
        found.join(", ")
    };
    line(tf(
        l10n,
        if found.is_empty() {
            "dev-pe-candidates-none"
        } else {
            "dev-pe-candidates"
        },
        &[("list", &list)],
    ));

    line(String::new());
    line(tf(
        l10n,
        "dev-pe-dllchars",
        &[
            ("value", &format!("{:#06x}", r.dll_characteristics)),
            (
                "names",
                &flag_names(r.dll_characteristics.into(), DLL_CHARACTERISTICS),
            ),
        ],
    ));
    match &r.load_config {
        None => line(l10n.t("dev-pe-loadconfig-absent")),
        Some(lc) => {
            line(tf(
                l10n,
                "dev-pe-loadconfig",
                &[("size", &lc.size.to_string())],
            ));
            match lc.dependent_load_flags {
                None => line(l10n.t("dev-pe-dlf-short")),
                Some(0) => line(l10n.t("dev-pe-dlf-zero")),
                Some(v) => line(tf(
                    l10n,
                    "dev-pe-dlf",
                    &[
                        ("value", &format!("{v:#06x}")),
                        ("names", &flag_names(v.into(), DEPENDENT_LOAD_FLAGS)),
                    ],
                )),
            }
        }
    }
    line(l10n.t("dev-pe-search-note"));

    line(String::new());
    let callbacks = r
        .tls_callbacks
        .iter()
        .map(|v| hex(*v))
        .collect::<Vec<_>>()
        .join(", ");
    line(tf(
        l10n,
        "dev-pe-tls",
        &[
            ("present", &yes_no(r.has_tls)),
            ("count", &r.tls_callbacks.len().to_string()),
            ("list", &callbacks),
        ],
    ));

    line(String::new());
    line(tf(
        l10n,
        "dev-pe-sections",
        &[("count", &r.sections.len().to_string())],
    ));
    line(l10n.t("dev-pe-sections-head"));
    for s in &r.sections {
        line(format!(
            "  {:<8} {:>10} {:>10} {:>10} {:>10}  {}",
            s.name,
            hex(s.virtual_address.into()),
            hex(s.virtual_size.into()),
            hex(s.raw_offset.into()),
            hex(s.raw_size.into()),
            section_flags(s.characteristics),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(buf: &mut [u8], at: usize, bytes: &[u8]) {
        buf[at..at + bytes.len()].copy_from_slice(bytes);
    }

    /// A tiny synthetic PE32+ image: `.text` and `.rdata`; imports `VERSION.dll`; a Load
    /// Configuration directory with `DependentLoadFlags` = SEARCH_SYSTEM32; no real code.
    fn synthetic_pe() -> Vec<u8> {
        let mut b = vec![0u8; 0x800];
        put(&mut b, 0, b"MZ");
        put(&mut b, 0x3c, &0x40u32.to_le_bytes());
        put(&mut b, 0x40, b"PE\0\0");
        // COFF header: machine, 2 sections, optional header size 0xF0.
        put(&mut b, 0x44, &0x8664u16.to_le_bytes());
        put(&mut b, 0x46, &2u16.to_le_bytes());
        put(&mut b, 0x54, &0xF0u16.to_le_bytes());
        put(&mut b, 0x56, &0x0022u16.to_le_bytes());
        // Optional header (PE32+) at 0x58.
        let o = 0x58;
        put(&mut b, o, &0x20bu16.to_le_bytes());
        put(&mut b, o + 16, &0x1000u32.to_le_bytes()); // entry point
        put(&mut b, o + 24, &0x1_4000_0000u64.to_le_bytes()); // image base
        put(&mut b, o + 32, &0x1000u32.to_le_bytes()); // section alignment
        put(&mut b, o + 36, &0x200u32.to_le_bytes()); // file alignment
        put(&mut b, o + 56, &0x3000u32.to_le_bytes()); // size of image
        put(&mut b, o + 60, &0x200u32.to_le_bytes()); // size of headers
        put(&mut b, o + 68, &2u16.to_le_bytes()); // GUI
        put(&mut b, o + 70, &0x8160u16.to_le_bytes()); // dll characteristics
        put(&mut b, o + 108, &16u32.to_le_bytes()); // data directory count
        let dirs = o + 112;
        put(&mut b, dirs + 8, &0x2000u32.to_le_bytes()); // import RVA
        put(&mut b, dirs + 12, &0x28u32.to_le_bytes());
        put(&mut b, dirs + 80, &0x2200u32.to_le_bytes()); // load config RVA
        put(&mut b, dirs + 84, &0x58u32.to_le_bytes());
        // Section table at 0x58 + 0xF0 = 0x148.
        let s = 0x148;
        put(&mut b, s, b".text\0\0\0");
        put(&mut b, s + 8, &0x10u32.to_le_bytes());
        put(&mut b, s + 12, &0x1000u32.to_le_bytes());
        put(&mut b, s + 16, &0x200u32.to_le_bytes());
        put(&mut b, s + 20, &0x200u32.to_le_bytes());
        put(&mut b, s + 36, &0x6000_0020u32.to_le_bytes());
        let s2 = s + 40;
        put(&mut b, s2, b".rdata\0\0");
        put(&mut b, s2 + 8, &0x300u32.to_le_bytes());
        put(&mut b, s2 + 12, &0x2000u32.to_le_bytes());
        put(&mut b, s2 + 16, &0x400u32.to_le_bytes());
        put(&mut b, s2 + 20, &0x400u32.to_le_bytes());
        put(&mut b, s2 + 36, &0x4000_0040u32.to_le_bytes());
        // `.rdata` raw data at 0x400 <-> RVA 0x2000.
        let r = 0x400;
        // Import descriptor: ILT at 0x2060, name at 0x2100, IAT at 0x2080; then a null descriptor.
        put(&mut b, r, &0x2060u32.to_le_bytes());
        put(&mut b, r + 12, &0x2100u32.to_le_bytes());
        put(&mut b, r + 16, &0x2080u32.to_le_bytes());
        // ILT: one by-name thunk to a hint/name at 0x2120, then the null terminator.
        put(&mut b, r + 0x60, &0x2120u64.to_le_bytes());
        put(&mut b, r + 0x100, b"VERSION.dll\0");
        put(&mut b, r + 0x122, b"GetFileVersionInfoW\0");
        // Load Configuration at 0x2200: size 0x58, DependentLoadFlags (+0x50) = 0x800.
        put(&mut b, r + 0x200, &0x58u32.to_le_bytes());
        put(&mut b, r + 0x200 + 0x50, &0x0800u16.to_le_bytes());
        b
    }

    #[test]
    fn reads_headers_imports_load_config_and_sections() {
        let report = parse(&synthetic_pe()).unwrap();
        assert!(report.is_64);
        assert_eq!(report.machine, pe::IMAGE_FILE_MACHINE_AMD64);
        assert_eq!(report.image_base, 0x1_4000_0000);
        assert_eq!(report.entry_rva, 0x1000);
        assert_eq!(report.dll_characteristics, 0x8160);
        assert_eq!(report.imports.len(), 1);
        assert_eq!(report.imports[0].name, "VERSION.dll");
        assert_eq!(report.imports[0].functions, ["GetFileVersionInfoW"]);
        assert!(report.delay_imports.is_empty());
        assert_eq!(
            report.load_config,
            Some(LoadConfig {
                size: 0x58,
                dependent_load_flags: Some(0x0800)
            })
        );
        assert!(!report.has_tls && !report.has_clr);
        let names: Vec<_> = report.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, [".text", ".rdata"]);
        assert_eq!(report.sections[1].virtual_address, 0x2000);
        assert_eq!(report.sections[1].raw_offset, 0x400);
    }

    #[test]
    fn candidates_match_case_insensitively() {
        assert!(is_candidate("VERSION.dll"));
        assert!(is_candidate("WinHTTP.DLL"));
        assert!(!is_candidate("kernel32.dll"));
    }

    #[test]
    fn short_load_config_has_no_dependent_load_flags() {
        let lc = load_config(&[0x40, 0, 0, 0], true).unwrap();
        assert_eq!(lc.dependent_load_flags, None);
        assert!(load_config(&[0, 0], true).is_err());
    }

    #[test]
    fn flag_names_lists_set_bits() {
        assert_eq!(
            flag_names(0x0a00, DEPENDENT_LOAD_FLAGS),
            "SEARCH_APPLICATION_DIR | SEARCH_SYSTEM32"
        );
        assert_eq!(flag_names(0, DEPENDENT_LOAD_FLAGS), "-");
    }

    #[test]
    fn rejects_non_pe_data() {
        assert!(parse(b"not a pe file at all").is_err());
    }
}
