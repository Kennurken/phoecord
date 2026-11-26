//! REST route helpers shared by the proxy and plugins.

/// `/api/v9/channels/1/messages` → `/channels/1/messages`; other paths stay as they are.
pub fn route_of(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("/api/") {
        if let Some((ver, tail)) = rest.split_once('/')
            && ver.len() > 1
            && ver.starts_with('v')
            && ver[1..].chars().all(|c| c.is_ascii_digit())
        {
            return format!("/{tail}");
        }
        return format!("/{rest}");
    }
    path.to_string()
}

/// `/api/v9/gifs/search?q=cat` → (`/api/v9/gifs/search`, `Some("q=cat")`). An empty query
/// (`/x?`) is `None`, as is a path without `?`.
pub fn split_query(path: &str) -> (&str, Option<&str>) {
    match path.split_once('?') {
        Some((p, q)) if !q.is_empty() => (p, Some(q)),
        Some((p, _)) => (p, None),
        None => (path, None),
    }
}

/// `q=cat%20gif&limit=5` → `[("q", "cat gif"), ("limit", "5")]`, as an HTML form encodes a
/// query: `+` is a space, `%XX` a byte, and bytes that are not UTF-8 become U+FFFD. A pair
/// without `=` has an empty value; empty pairs (`a=1&&b=2`) are skipped.
pub fn query_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (form_decode(k), form_decode(v))
        })
        .collect()
}

fn form_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Channel id from `/channels/{id}/messages` (and `/channels/{id}/messages/{mid}`).
pub fn message_route_channel(route: &str) -> Option<u64> {
    let seg: Vec<&str> = route.trim_matches('/').split('/').collect();
    match seg.as_slice() {
        ["channels", id, "messages"] | ["channels", id, "messages", _] => id.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        assert_eq!(
            route_of("/api/v9/channels/1/messages"),
            "/channels/1/messages"
        );
        assert_eq!(route_of("/api/science"), "/science");
        assert_eq!(route_of("/assets/app.js"), "/assets/app.js");
        assert_eq!(message_route_channel("/channels/42/messages"), Some(42));
        assert_eq!(message_route_channel("/channels/42/messages/7"), Some(42));
        assert_eq!(message_route_channel("/channels/42/typing"), None);
    }

    #[test]
    fn queries() {
        assert_eq!(
            split_query("/api/v9/gifs/search?q=cat&limit=5"),
            ("/api/v9/gifs/search", Some("q=cat&limit=5"))
        );
        assert_eq!(split_query("/gifs/search?"), ("/gifs/search", None));
        assert_eq!(split_query("/gifs/search"), ("/gifs/search", None));
        assert_eq!(
            query_pairs("q=cat+gif%21&media_format=mp4&&locale=ru%D0%"),
            vec![
                ("q".to_string(), "cat gif!".to_string()),
                ("media_format".to_string(), "mp4".to_string()),
                ("locale".to_string(), "ru\u{FFFD}%".to_string()),
            ]
        );
        assert_eq!(
            query_pairs("flag"),
            vec![("flag".to_string(), String::new())]
        );
        assert_eq!(
            query_pairs("q=%D0%BA%D0%BE%D1%82"),
            vec![("q".to_string(), "кот".to_string())]
        );
    }
}
