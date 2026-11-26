//! REST bridge: what the `send` entry hook and the `poll` vtable hook hand to `Engine`, and the
//! verdict they get back.
//!
//! The hooks read method, URL and body out of the client's structures (ABI work that waits for the
//! 2.1.5 findings); this module is the part after that, in plain Rust: JSON in, verdict out.

use phoecord_core::Val;
use phoecord_transport::{Effects, Engine, RequestOutcome};

/// What the hook does with the call it intercepted.
#[derive(Debug, PartialEq)]
pub enum HttpAction {
    /// Let it through untouched.
    Pass,
    /// Continue with this body instead of the original (the caller allocates it with the host
    /// allocator).
    ReplaceBody(Vec<u8>),
    /// Answer locally: the request never reaches Discord.
    Reply { status: u16, body: Option<Vec<u8>> },
}

#[derive(Debug)]
pub struct HttpOutcome {
    pub action: HttpAction,
    /// What plugins queued; delivery belongs to the transport (2.2.7).
    pub effects: Effects,
}

impl HttpOutcome {
    fn pass(effects: Effects) -> HttpOutcome {
        HttpOutcome {
            action: HttpAction::Pass,
            effects,
        }
    }
}

/// `https://discord.com/api/v9/channels/1/messages?limit=5` → `/channels/1/messages`.
pub fn route_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let path = rest.find('/').map_or("/", |i| &rest[i..]);
    let path = path.split(['?', '#']).next().unwrap_or("/");
    let path = path.strip_prefix("/api").unwrap_or(path);
    let path = match path.strip_prefix("/v") {
        Some(r) if r.split('/').next().is_some_and(is_version) => {
            r.find('/').map_or("/", |i| &r[i..])
        }
        _ => path,
    };
    if path.is_empty() {
        "/".into()
    } else {
        path.into()
    }
}

/// `https://discord.com/api/v9/gifs/search?q=cat#x` → `Some("q=cat")`; `None` without a query.
/// Plugins read it with `Host::request_query`; `route_of` drops it.
pub fn query_of(url: &str) -> Option<&str> {
    let url = url.split('#').next().unwrap_or("");
    let q = url.split_once('?')?.1;
    (!q.is_empty()).then_some(q)
}

fn is_version(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn encode(v: &Val) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&v.to_json()).map_err(|e| format!("cannot encode body: {e}"))
}

/// A REST request on its way to Discord.
pub fn request(
    engine: &Engine,
    method: &str,
    url: &str,
    body: Option<&[u8]>,
    can_replace_body: bool,
) -> Result<HttpOutcome, String> {
    let route = route_of(url);
    let query = query_of(url);
    // A body that is not JSON (an upload) is left alone: plugins see none and cannot change it.
    let (mut val, opaque) = match body {
        None | Some([]) => (None, false),
        Some(b) => match serde_json::from_slice(b) {
            Ok(v) => (Some(Val::from_json(v)), false),
            Err(_) => (None, true),
        },
    };
    let (outcome, effects) = engine.on_request_with_query(method, &route, query, &mut val);
    match outcome {
        RequestOutcome::Forward { changed } => {
            if !changed || opaque {
                return Ok(HttpOutcome::pass(effects));
            }
            let Some(v) = val.filter(|_| can_replace_body) else {
                return Err(format!(
                    "{method} {route}: a plugin changed the body but body replacement is off"
                ));
            };
            Ok(HttpOutcome {
                action: HttpAction::ReplaceBody(encode(&v)?),
                effects,
            })
        }
        RequestOutcome::Reply { status, body } => Ok(HttpOutcome {
            action: HttpAction::Reply {
                status,
                body: body.as_ref().map(encode).transpose()?,
            },
            effects,
        }),
    }
}

/// A REST answer on its way back to the client.
pub fn response(
    engine: &Engine,
    method: &str,
    url: &str,
    status: u16,
    body: &[u8],
    can_replace_body: bool,
) -> Result<HttpOutcome, String> {
    let route = route_of(url);
    let query = query_of(url);
    if !engine.wants_response(method, &route) {
        return Ok(HttpOutcome::pass(Effects::default()));
    }
    let json = serde_json::from_slice(body)
        .map_err(|e| format!("{method} {route}: response body is not JSON ({e})"))?;
    let mut val = Val::from_json(json);
    let (changed, effects) = engine.on_response_with_query(method, &route, query, status, &mut val);
    if !changed {
        return Ok(HttpOutcome::pass(effects));
    }
    if !can_replace_body {
        return Err(format!(
            "{method} {route}: a plugin changed the response but body replacement is off"
        ));
    }
    Ok(HttpOutcome {
        action: HttpAction::ReplaceBody(encode(&val)?),
        effects,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_lose_host_version_and_query() {
        assert_eq!(
            route_of("https://discord.com/api/v9/channels/1/messages?limit=5"),
            "/channels/1/messages"
        );
        assert_eq!(route_of("/api/v10/science"), "/science");
        assert_eq!(route_of("https://discord.com/api/v9"), "/");
        assert_eq!(route_of("https://cdn.example.com/a/b#x"), "/a/b");
        assert_eq!(route_of("https://discord.com"), "/");
        assert_eq!(route_of("/v/x"), "/v/x");
        assert_eq!(
            query_of("https://discord.com/api/v9/gifs/search?q=cat&limit=5#x"),
            Some("q=cat&limit=5")
        );
        assert_eq!(query_of("https://discord.com/api/v9/gifs/search?"), None);
        assert_eq!(query_of("https://discord.com/a#frag?x=1"), None);
        assert_eq!(query_of("https://discord.com/api/v9/users/@me"), None);
    }
}
