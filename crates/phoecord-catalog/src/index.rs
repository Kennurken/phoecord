//! The catalog index: `catalog/plugins.json` in the PhoeCord repository. Entries are added by pull
//! request and reviewed like code (`catalog/README.md`); this module reads the file and checks every
//! field, so a bad entry is rejected both in CI and on the user's machine.

use crate::store::{Error, Installed};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// The only index format there is.
pub const SCHEMA: u32 = 1;
/// The index is refused when larger than this.
pub const MAX_INDEX_BYTES: u64 = 1024 * 1024;
/// File inside `plugins/<id>/` that says the package came from the catalog.
pub const MARK_FILE: &str = ".catalog.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// snake_case, the same as `id` in the package's `plugin.toml`.
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    /// Semver, the same as `version` in `plugin.toml`.
    pub version: String,
    /// HTTPS link to a `.zip` with `plugin.toml` and `plugin.wasm`.
    pub download: String,
    /// Lower-case hex SHA-256 of the downloaded file.
    pub sha256: String,
    /// The rights the plugin asks for; `plugin.toml` may not ask for more.
    pub rights: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    #[serde(rename = "$schema", default, skip_serializing)]
    pub schema_ref: Option<String>,
    pub schema: u32,
    pub plugins: Vec<Entry>,
}

/// Where a catalog-installed package came from, written next to it so updates can be offered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub version: String,
    pub sha256: String,
}

impl Mark {
    pub fn of(e: &Entry) -> Mark {
        Mark {
            version: e.version.clone(),
            sha256: e.sha256.clone(),
        }
    }

    pub fn write(&self, package_dir: &Path) -> Result<(), Error> {
        let text = serde_json::to_string_pretty(self).map_err(|e| Error::Io(e.to_string()))?;
        std::fs::write(package_dir.join(MARK_FILE), text)?;
        Ok(())
    }

    pub fn read(package_dir: &Path) -> Option<Mark> {
        let text = std::fs::read_to_string(package_dir.join(MARK_FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }
}

/// A newer version in the catalog than the installed one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub id: String,
    pub from: String,
    pub to: String,
}

fn valid_id(id: &str) -> bool {
    id.chars().next().is_some_and(|f| f.is_ascii_lowercase())
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

impl Entry {
    /// Every rule of `catalog/plugins.schema.json` that JSON Schema cannot say.
    ///
    /// `strict` (the repository check) wants an `https://` link; otherwise a link to this computer
    /// is accepted too, which lets tests and local mirrors run against a local server.
    pub fn check(&self, strict: bool) -> Result<(), String> {
        let id = &self.id;
        if !valid_id(id) {
            return Err(format!(
                "{id:?}: id must be snake_case (a-z, 0-9, _), starting with a letter"
            ));
        }
        let bad = |what: &str, why: &str| Err(format!("{id}: {what} {why}"));
        for (what, v, max) in [
            ("name", &self.name, 64),
            ("author", &self.author, 64),
            ("description", &self.description, 300),
        ] {
            if v.trim().is_empty() {
                return bad(what, "is empty");
            }
            if v.chars().count() > max || v.chars().any(char::is_control) {
                return bad(
                    what,
                    &format!("is longer than {max} characters or has control characters"),
                );
            }
        }
        if semver::Version::parse(&self.version).is_err() {
            return bad("version", "is not a semantic version like 1.2.3");
        }
        let link_ok = crate::store::is_https(&self.download)
            || (!strict && crate::net::is_loopback_http(&self.download));
        if !link_ok || self.download.chars().any(char::is_whitespace) {
            return bad("download", "must be an https:// link without spaces");
        }
        if self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return bad("sha256", "must be 64 lower-case hex digits");
        }
        let mut seen = HashSet::new();
        for r in &self.rights {
            if !phoecord_plugin_runtime::wasm::Right::ALL
                .iter()
                .any(|k| k.name() == r)
            {
                return bad("rights", &format!("has an unknown right {r:?}"));
            }
            if !seen.insert(r) {
                return bad("rights", &format!("lists {r:?} twice"));
            }
        }
        Ok(())
    }
}

impl Index {
    pub fn empty() -> Index {
        Index {
            schema_ref: None,
            schema: SCHEMA,
            plugins: Vec::new(),
        }
    }

    /// Parse and validate the index file as a client does.
    pub fn parse(text: &str) -> Result<Index, String> {
        Self::parse_with(text, false)
    }

    /// The same, as the repository's CI check wants it: every download is an `https://` link.
    pub fn parse_strict(text: &str) -> Result<Index, String> {
        Self::parse_with(text, true)
    }

    fn parse_with(text: &str, strict: bool) -> Result<Index, String> {
        let i: Index = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if i.schema != SCHEMA {
            return Err(format!(
                "index format {} is not supported (this PhoeCord reads {SCHEMA}); update PhoeCord",
                i.schema
            ));
        }
        let mut ids = HashSet::new();
        for e in &i.plugins {
            e.check(strict)?;
            if !ids.insert(e.id.as_str()) {
                return Err(format!("{}: listed twice", e.id));
            }
        }
        Ok(i)
    }

    pub fn find(&self, id: &str) -> Option<&Entry> {
        self.plugins.iter().find(|e| e.id == id)
    }

    /// Installed catalog packages for which the index has a newer version.
    pub fn updates(&self, installed: &[Installed]) -> Vec<Update> {
        installed
            .iter()
            .filter_map(|p| {
                let have = p.manifest.as_ref()?.manifest.version.clone();
                // Only what was installed from the catalog: a same-named package from elsewhere
                // is never replaced by a catalog entry unless the user asks for it.
                Mark::read(&p.dir)?;
                let e = self.find(&p.id)?;
                is_newer(&e.version, &have).then(|| Update {
                    id: p.id.clone(),
                    from: have,
                    to: e.version.clone(),
                })
            })
            .collect()
    }
}

/// Is `new` a higher version than `old`? An unparseable `old` counts as older.
pub fn is_newer(new: &str, old: &str) -> bool {
    match (semver::Version::parse(new), semver::Version::parse(old)) {
        (Ok(n), Ok(o)) => n > o,
        (Ok(_), Err(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"N","author":"A","description":"D","version":"1.0.0","download":"https://example.com/{id}.zip","sha256":"{}","rights":["notify"]}}"#,
            "a".repeat(64)
        )
    }

    fn index(plugins: &str) -> String {
        format!(r#"{{"schema":1,"plugins":[{plugins}]}}"#)
    }

    #[test]
    fn a_good_index_parses_and_an_empty_one_too() {
        let i = Index::parse(&index(&entry("clear_urls"))).unwrap();
        assert_eq!(i.find("clear_urls").unwrap().rights, vec!["notify"]);
        assert!(Index::parse(&index("")).unwrap().plugins.is_empty());
        // The `$schema` line editors use is allowed.
        let with_ref = r#"{"$schema":"./plugins.schema.json","schema":1,"plugins":[]}"#;
        assert!(Index::parse(with_ref).is_ok());
    }

    #[test]
    fn strict_mode_wants_https_even_for_this_computer() {
        let local = index(&entry("p1")).replace("https://example.com", "http://127.0.0.1:8080");
        assert!(Index::parse(&local).is_ok());
        assert!(Index::parse_strict(&local).is_err());
        let evil =
            index(&entry("p1")).replace("https://example.com", "http://127.0.0.1@example.com");
        assert!(Index::parse(&evil).is_err());
    }

    #[test]
    fn bad_entries_are_refused() {
        let bad = |from: &str, to: &str| {
            let text = index(&entry("p1")).replace(from, to);
            assert!(Index::parse(&text).is_err(), "{to}");
        };
        bad(r#""p1""#, r#""P-1""#);
        bad("1.0.0", "one");
        bad("https://example.com", "http://example.com");
        bad("https://example.com", "ftp://example.com");
        bad(&"a".repeat(64), &"A".repeat(64));
        bad(&"a".repeat(64), &"a".repeat(63));
        bad("notify", "root");
        bad(r#""N""#, r#""""#);
        bad(r#""schema":1"#, r#""schema":2"#);
        assert!(Index::parse(&index(&format!("{},{}", entry("p1"), entry("p1")))).is_err());
        assert!(Index::parse(&index(&entry("p1").replace('}', r#","extra":1}"#))).is_err());
        assert!(Index::parse("not json").is_err());
    }

    #[test]
    fn newer_means_a_higher_semver() {
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.1"));
        assert!(is_newer("1.0.0", "1.0.0-beta.1"));
        assert!(is_newer("1.0.0", "garbage"));
        assert!(!is_newer("garbage", "1.0.0"));
    }
}

/// The repository's own `catalog/plugins.json` passes the strict check; this is the CI check for
/// pull requests that add or change an entry.
#[cfg(test)]
mod repository_file {
    use super::*;

    #[test]
    fn catalog_plugins_json_is_valid() {
        let text = include_str!("../../../catalog/plugins.json");
        let i = Index::parse_strict(text).unwrap_or_else(|e| panic!("catalog/plugins.json: {e}"));
        let mut sorted: Vec<&str> = i.plugins.iter().map(|e| e.id.as_str()).collect();
        let before = sorted.clone();
        sorted.sort_unstable();
        assert_eq!(
            before, sorted,
            "catalog/plugins.json: keep entries sorted by id"
        );
    }

    #[test]
    fn schema_file_agrees_with_the_rights_the_runtime_knows() {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("../../../catalog/plugins.schema.json")).unwrap();
        let listed: Vec<&str> = schema["$defs"]["entry"]["properties"]["rights"]["items"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for r in phoecord_plugin_runtime::wasm::Right::ALL {
            assert!(
                listed.contains(&r.name()),
                "schema lacks right {}",
                r.name()
            );
        }
        assert_eq!(
            listed.len(),
            phoecord_plugin_runtime::wasm::Right::ALL.len()
        );
    }
}
