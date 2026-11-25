//! Using the catalog: fetch the index, download an entry and refuse it unless its SHA-256 is the
//! listed one, then hand it to the normal staging so the user still sees the rights dialog.

use crate::index::{Entry, Index, MAX_INDEX_BYTES, Mark};
use crate::net;
use crate::store::{self, Candidate, Error, Store, Verify};
use sha2::{Digest, Sha256};

/// The index in the PhoeCord repository, as served from `main`.
pub const DEFAULT_URL: &str =
    "https://raw.githubusercontent.com/Nanda070/phoecord/main/catalog/plugins.json";
/// Overrides [`DEFAULT_URL`] (a mirror, or a test server). Plain `http://` is accepted only for
/// this computer.
pub const URL_ENV: &str = "PHOECORD_CATALOG_URL";

/// The index URL to use: `PHOECORD_CATALOG_URL`, else [`DEFAULT_URL`].
pub fn index_url() -> String {
    std::env::var(URL_ENV)
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

/// Fetch and validate the index at `url`.
pub fn fetch_index(url: &str) -> Result<Index, Error> {
    let bytes = net::get(url, MAX_INDEX_BYTES, true)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::Index("not UTF-8 text".into()))?;
    Index::parse(text).map_err(Error::Index)
}

/// Lower-case hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Download `entry`'s zip and check it against the listed hash.
pub fn download_entry(entry: &Entry) -> Result<Vec<u8>, Error> {
    let bytes = net::get(&entry.download, store::MAX_BYTES, true)?;
    if sha256_hex(&bytes) != entry.sha256 {
        return Err(Error::HashMismatch);
    }
    Ok(bytes)
}

/// Download, hash-check and stage `entry`. The staged package must be the one the entry
/// describes: same id and version, and no right the entry does not list. Nothing is installed
/// until [`Store::commit`].
pub fn stage_entry(store: &Store, entry: &Entry, verify: &Verify) -> Result<Candidate, Error> {
    let bytes = download_entry(entry)?;
    let mut c = store.stage_zip(&bytes, verify)?;
    let m = &c.manifest;
    let problem = if m.manifest.id != entry.id {
        Some(format!(
            "plugin.toml has id {}, the catalog lists {}",
            m.manifest.id, entry.id
        ))
    } else if m.manifest.version != entry.version {
        Some(format!(
            "plugin.toml has version {}, the catalog lists {}",
            m.manifest.version, entry.version
        ))
    } else {
        m.rights
            .iter()
            .find(|r| !entry.rights.contains(r))
            .map(|r| format!("plugin.toml asks for the right {r}, which the catalog does not list"))
    };
    if let Some(why) = problem {
        store.discard(&c);
        return Err(Error::CatalogMismatch(why));
    }
    c.from_catalog = Some(Mark::of(entry));
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    /// A local HTTP server: path -> body, 404 for anything else. Stops when dropped.
    struct Fixture {
        base: String,
        routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }

    impl Fixture {
        fn start() -> Fixture {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let routes: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::default();
            let r = routes.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut s) = stream else { break };
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match s.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let head = String::from_utf8_lossy(&buf).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let body = r.lock().unwrap().get(&path).cloned();
                    let (status, body) = match body {
                        Some(b) => ("200 OK", b),
                        None => ("404 Not Found", b"no".to_vec()),
                    };
                    let _ = write!(
                        s,
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(&body);
                }
            });
            Fixture { base, routes }
        }

        fn serve(&self, path: &str, body: Vec<u8>) {
            self.routes.lock().unwrap().insert(path.into(), body);
        }

        fn url(&self, path: &str) -> String {
            format!("{}{path}", self.base)
        }
    }

    fn toml(version: &str, rights: &str) -> String {
        format!(
            "id = \"hello_wasm\"\nname = \"Hello\"\nversion = \"{version}\"\ndescription = \"Hi\"\nauthors = [\"Aigerim\"]\napi = 1\nrights = [{rights}]\n"
        )
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn package(version: &str, rights: &str) -> Vec<u8> {
        zip_of(&[
            ("plugin.toml", toml(version, rights).as_bytes()),
            ("plugin.wasm", version.as_bytes()),
        ])
    }

    fn ok(_: &Path) -> Result<(), String> {
        Ok(())
    }

    /// Serve `zip` and an index listing it (with its real hash, unless `sha` overrides it).
    fn publish(
        fx: &Fixture,
        version: &str,
        zip: &[u8],
        sha: Option<String>,
        rights: &[&str],
    ) -> Entry {
        fx.serve("/hello.zip", zip.to_vec());
        let entry = Entry {
            id: "hello_wasm".into(),
            name: "Hello".into(),
            author: "Aigerim".into(),
            description: "Hi".into(),
            version: version.into(),
            download: fx.url("/hello.zip"),
            sha256: sha.unwrap_or_else(|| sha256_hex(zip)),
            rights: rights.iter().map(ToString::to_string).collect(),
        };
        let index = Index {
            schema_ref: None,
            schema: 1,
            plugins: vec![entry.clone()],
        };
        fx.serve("/plugins.json", serde_json::to_vec(&index).unwrap());
        entry
    }

    fn store(t: &Path) -> Store {
        Store::new(&t.join("data"), &t.join("config.toml"))
    }

    #[test]
    fn index_is_fetched_over_loopback_http_only() {
        let fx = Fixture::start();
        publish(
            &fx,
            "1.0.0",
            &package("1.0.0", "\"notify\""),
            None,
            &["notify"],
        );
        let i = fetch_index(&fx.url("/plugins.json")).unwrap();
        assert_eq!(i.plugins.len(), 1);
        assert!(matches!(
            fetch_index(&fx.url("/missing.json")),
            Err(Error::Download(_))
        ));
        fx.serve("/bad.json", b"{\"schema\":1,\"plugins\":[{}]}".to_vec());
        assert!(matches!(
            fetch_index(&fx.url("/bad.json")),
            Err(Error::Index(_))
        ));
        // A plain-http link to anywhere else is refused before any connection.
        assert_eq!(
            fetch_index("http://example.com/plugins.json").unwrap_err(),
            Error::NotHttps
        );
        assert_eq!(
            fetch_index("http://127.0.0.1@example.com/p.json").unwrap_err(),
            Error::NotHttps
        );
    }

    #[test]
    fn good_hash_installs_with_the_rights_shown_and_marks_the_package() {
        let fx = Fixture::start();
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let e = publish(
            &fx,
            "1.0.0",
            &package("1.0.0", "\"notify\""),
            None,
            &["notify"],
        );
        let c = stage_entry(&s, &e, &ok).unwrap();
        // What the confirm dialog / prompt shows.
        assert_eq!(c.asks.rights, vec!["notify"]);
        assert!(s.list().is_empty(), "nothing lands before the yes");
        s.commit(&c).unwrap();
        let installed = s.list();
        assert_eq!(installed.len(), 1);
        assert_eq!(
            crate::index::Mark::read(&installed[0].dir).unwrap().version,
            "1.0.0"
        );
        assert!(!s.staging.exists());
    }

    #[test]
    fn tampered_download_is_refused_and_nothing_is_staged() {
        let fx = Fixture::start();
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let good = package("1.0.0", "\"notify\"");
        let e = publish(&fx, "1.0.0", &good, None, &["notify"]);
        // The server now serves other bytes than the ones the index was reviewed with.
        fx.serve("/hello.zip", package("1.0.0", "\"notify\", \"rest\""));
        assert_eq!(stage_entry(&s, &e, &ok).unwrap_err(), Error::HashMismatch);
        assert!(!s.staging.exists());
        assert!(s.list().is_empty());
        // A wrong hash in the index is refused the same way.
        let e = publish(&fx, "1.0.0", &good, Some("0".repeat(64)), &["notify"]);
        assert_eq!(stage_entry(&s, &e, &ok).unwrap_err(), Error::HashMismatch);
    }

    #[test]
    fn a_package_that_does_not_match_its_entry_is_refused() {
        let fx = Fixture::start();
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        // Asks for a right the entry did not list.
        let e = publish(
            &fx,
            "1.0.0",
            &package("1.0.0", "\"notify\", \"rest\""),
            None,
            &["notify"],
        );
        assert!(matches!(
            stage_entry(&s, &e, &ok).unwrap_err(),
            Error::CatalogMismatch(_)
        ));
        // A different version than listed.
        let e = publish(
            &fx,
            "2.0.0",
            &package("1.0.0", "\"notify\""),
            None,
            &["notify"],
        );
        assert!(matches!(
            stage_entry(&s, &e, &ok).unwrap_err(),
            Error::CatalogMismatch(_)
        ));
        assert!(!s.staging.exists());
    }

    #[test]
    fn newer_version_is_offered_installs_over_the_old_one_and_uninstalls() {
        let fx = Fixture::start();
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let e = publish(
            &fx,
            "1.0.0",
            &package("1.0.0", "\"notify\""),
            None,
            &["notify"],
        );
        s.commit(&stage_entry(&s, &e, &ok).unwrap()).unwrap();
        let index = fetch_index(&fx.url("/plugins.json")).unwrap();
        assert!(index.updates(&s.list()).is_empty());

        let e = publish(
            &fx,
            "1.1.0",
            &package("1.1.0", "\"notify\""),
            None,
            &["notify"],
        );
        let index = fetch_index(&fx.url("/plugins.json")).unwrap();
        let ups = index.updates(&s.list());
        assert_eq!(ups.len(), 1);
        assert_eq!(
            (ups[0].from.as_str(), ups[0].to.as_str()),
            ("1.0.0", "1.1.0")
        );

        let c = stage_entry(&s, &e, &ok).unwrap();
        assert_eq!(c.replaces.as_deref(), Some("1.0.0"));
        s.commit(&c).unwrap();
        assert_eq!(
            std::fs::read(s.root.join("hello_wasm/plugin.wasm")).unwrap(),
            b"1.1.0"
        );
        assert!(index.updates(&s.list()).is_empty());

        s.remove("hello_wasm").unwrap();
        assert!(s.list().is_empty());
    }

    #[test]
    fn a_hand_installed_package_is_not_offered_a_catalog_update() {
        let fx = Fixture::start();
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let hand = zip_of(&[
            ("plugin.toml", toml("0.1.0", "").as_bytes()),
            ("plugin.wasm", b"mine"),
        ]);
        s.commit(&s.stage_zip(&hand, &ok).unwrap()).unwrap();
        publish(
            &fx,
            "1.0.0",
            &package("1.0.0", "\"notify\""),
            None,
            &["notify"],
        );
        let index = fetch_index(&fx.url("/plugins.json")).unwrap();
        assert!(index.updates(&s.list()).is_empty());
    }

    #[test]
    fn sha256_known_answer() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
