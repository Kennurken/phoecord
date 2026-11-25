//! One HTTP GET with a size cap: HTTPS only, except to this computer (tests and local mirrors).

use crate::store::Error;
use std::time::Duration;

/// Is `url` an `http://` link to 127.0.0.1, `[::1]` or `localhost`?
pub fn is_loopback_http(url: &str) -> bool {
    let u = url.trim();
    let Some(rest) = u.get(..7).filter(|p| p.eq_ignore_ascii_case("http://")) else {
        return false;
    };
    let _ = rest;
    let host = u[7..].split(['/', '?', '#']).next().unwrap_or_default();
    // `user@host` would make the real host something else.
    if host.contains('@') {
        return false;
    }
    let name = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    matches!(
        name.to_ascii_lowercase().as_str(),
        "127.0.0.1" | "::1" | "localhost"
    )
}

/// GET `url`, at most `limit` bytes. `allow_loopback`: plain `http://` is accepted when it goes
/// to this computer (and then no redirect is followed).
pub fn get(url: &str, limit: u64, allow_loopback: bool) -> Result<Vec<u8>, Error> {
    let local = allow_loopback && is_loopback_http(url);
    if !local && !crate::store::is_https(url) {
        return Err(Error::NotHttps);
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(!local)
        .max_redirects(if local { 0 } else { 5 })
        .timeout_global(Some(Duration::from_secs(60)))
        .build()
        .into();
    let mut resp = agent
        .get(url.trim())
        .call()
        .map_err(|e| Error::Download(e.to_string()))?;
    resp.body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => Error::TooBig,
            e => Error::Download(e.to_string()),
        })
}
