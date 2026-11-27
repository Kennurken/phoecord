//! Loader ↔ proxy IPC: the message types, the line codec and the retry schedule.
//!
//! The loader (inside the Discord client) is the *client* and `phoecord run` / `launch` is the
//! *server* of a per-user Windows named pipe ([`crate::pipe`]). This module has no OS code, so
//! both sides use it (the proxy depends on this crate with the `dll` feature off) and the tests run
//! on every OS. It is the one place that defines the protocol.
//!
//! Wire format: JSON lines, one message per line, UTF-8, `\n` terminated. Every message is an
//! object with a protocol version `v` and a `type`:
//!
//! | `type` | direction | fields |
//! |:---|:---|:---|
//! | `hello` | loader → proxy | `loader_version`, `client_version?`, `text_hash`, `pid`, `detect_ms` |
//! | `status` | loader → proxy | `ui` (bool), `reason?` (why it is off) |
//! | `settings.get` | loader → proxy | `id`, `key` |
//! | `settings.set` | loader → proxy | `id`, `key`, `value` (a JSON scalar) |
//! | `settings.value` | proxy → loader | `id`, `key`, `value?` (absent: unset), `error?` |
//! | `ui.event` | loader → proxy | `name`, `payload` (any JSON); `menu_action`: see [`menu_action`] |
//! | `menu.get` | loader → proxy | `id` |
//! | `menu.items` | proxy → loader | `id`, `items` ([`MenuItem`]: `plugin`, `entry`, `label`) |
//! | `plugins.get` | loader → proxy | `id` |
//! | `plugins.list` | proxy → loader | `id`, `plugins` ([`PluginInfo`]), `more` (another `plugins.list` with this `id` follows) |
//!
//! A line longer than [`MAX_LINE`] bytes, invalid JSON, an unknown `type` or another `v` is an
//! error, and the receiver drops the connection.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, BufRead, Read};

/// Version of the protocol, sent as `v` in every message.
pub const PROTOCOL_VERSION: u32 = 1;

/// The longest line (without the newline) either side accepts or sends.
pub const MAX_LINE: usize = 64 * 1024;

/// A message without its version; [`Envelope`] adds `v`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Msg {
    /// First message of a connection: who the loader is and which client build it sits in.
    #[serde(rename = "hello")]
    Hello {
        loader_version: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_version: Option<String>,
        /// SHA-256 of the client's `.text` section (lowercase hex).
        text_hash: String,
        pid: u32,
        /// How long build detection took.
        detect_ms: u64,
    },
    /// Whether the UI layer is on and, when it is off, why ("no signatures").
    #[serde(rename = "status")]
    Status {
        ui: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Ask for a setting of the loader's own section.
    #[serde(rename = "settings.get")]
    SettingsGet { id: u64, key: String },
    /// Store a setting of the loader's own section.
    #[serde(rename = "settings.set")]
    SettingsSet { id: u64, key: String, value: Value },
    /// The proxy's answer to `settings.get` and `settings.set`.
    #[serde(rename = "settings.value")]
    SettingsValue {
        id: u64,
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Something happened in the client UI. `menu_action` runs the plugin's menu entry (2.3.5);
    /// other names are logged.
    #[serde(rename = "ui.event")]
    UiEvent { name: String, payload: Value },
    /// Ask for the plugin entries of the message context menu (2.3.5).
    #[serde(rename = "menu.get")]
    MenuGet { id: u64 },
    /// The proxy's answer to `menu.get`: the entries of the plugins that run now.
    #[serde(rename = "menu.items")]
    MenuItems { id: u64, items: Vec<MenuItem> },
    /// Ask for every plugin the settings tab lists (2.3.3, on the proxy transport, where the
    /// loader has no engine of its own).
    #[serde(rename = "plugins.get")]
    PluginsGet { id: u64 },
    /// The proxy's answer to `plugins.get`, in as many lines as [`MAX_LINE`] needs: every line
    /// but the last has `more: true`.
    #[serde(rename = "plugins.list")]
    PluginsList {
        id: u64,
        plugins: Vec<PluginInfo>,
        #[serde(default)]
        more: bool,
    },
}

/// One plugin for the settings tab, its setting labels already in the user's language.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// A WASM package (else built in).
    #[serde(default)]
    pub package: bool,
    /// Turned off for this run by a panic.
    #[serde(default)]
    pub crashed: bool,
    /// The rights a package's `plugin.toml` asks for, as written there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rights: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<SettingInfo>,
}

/// One setting as `SettingSpec` describes it; `kind` is `SettingKind::name`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingInfo {
    pub key: String,
    pub kind: String,
    pub default: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Value>,
    /// The label, translated.
    pub label: String,
}

/// `plugins` as `plugins.list` replies to request `id`, each line at most [`MAX_LINE`] bytes. A
/// plugin that alone does not fit is left out (and named in the `Err` list, for the log).
pub fn plugins_lines(id: u64, plugins: Vec<PluginInfo>) -> (Vec<Msg>, Vec<String>) {
    let fits = |plugins: &[PluginInfo]| {
        encode_line(&Msg::PluginsList {
            id,
            plugins: plugins.to_vec(),
            more: true,
        })
        .is_ok()
    };
    let mut out: Vec<Vec<PluginInfo>> = vec![Vec::new()];
    let mut dropped = Vec::new();
    for p in plugins {
        if !fits(std::slice::from_ref(&p)) {
            dropped.push(p.id);
            continue;
        }
        let last = out.last_mut().expect("never empty");
        last.push(p);
        if !fits(last) {
            let p = last.pop().expect("just pushed");
            out.push(vec![p]);
        }
    }
    let n = out.len();
    let msgs = out
        .into_iter()
        .enumerate()
        .map(|(i, plugins)| Msg::PluginsList {
            id,
            plugins,
            more: i + 1 < n,
        })
        .collect();
    (msgs, dropped)
}

/// One plugin entry for a context menu, its label in the user's language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    pub plugin: String,
    pub entry: String,
    pub label: String,
    /// The menu it goes in: `message` (absent from older proxies), `user`, `channel`, `server`.
    #[serde(default = "message_menu")]
    pub menu: String,
}

fn message_menu() -> String {
    "message".into()
}

/// The ids of the menu a click was in; which ones are set depends on `menu`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Click {
    pub menu: String,
    pub user_id: Option<u64>,
    pub guild_id: Option<u64>,
    pub channel_id: Option<u64>,
    pub message_id: Option<u64>,
}

/// A click on a plugin's menu entry as a `ui.event`: name `menu_action`, payload
/// `{"plugin", "entry", "menu", "user_id"?, "guild_id"?, "channel_id"?, "message_id"?}` (ids as
/// strings, as in Discord's JSON).
pub fn menu_action(plugin: &str, entry: &str, click: &Click) -> Msg {
    let mut p = serde_json::Map::new();
    p.insert("plugin".into(), plugin.into());
    p.insert("entry".into(), entry.into());
    p.insert("menu".into(), click.menu.clone().into());
    for (k, v) in [
        ("user_id", click.user_id),
        ("guild_id", click.guild_id),
        ("channel_id", click.channel_id),
        ("message_id", click.message_id),
    ] {
        if let Some(v) = v {
            p.insert(k.into(), v.to_string().into());
        }
    }
    Msg::UiEvent {
        name: "menu_action".into(),
        payload: Value::Object(p),
    }
}

/// The fields of a `menu_action` payload: (plugin, entry, click). A payload without `menu` is a
/// message-menu click.
pub fn parse_menu_action(payload: &Value) -> Option<(String, String, Click)> {
    let s = |k: &str| payload.get(k)?.as_str().map(str::to_string);
    let id = |k: &str| payload.get(k)?.as_str()?.parse::<u64>().ok();
    let click = Click {
        menu: s("menu").unwrap_or_else(message_menu),
        user_id: id("user_id"),
        guild_id: id("guild_id"),
        channel_id: id("channel_id"),
        message_id: id("message_id"),
    };
    Some((s("plugin")?, s("entry")?, click))
}

/// A message on the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    #[serde(flatten)]
    pub msg: Msg,
}

/// Why a line was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum LineError {
    TooLong,
    NotUtf8OrJson(String),
    Version(u32),
}

impl std::fmt::Display for LineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LineError::TooLong => write!(f, "line longer than {MAX_LINE} bytes"),
            LineError::NotUtf8OrJson(e) => write!(f, "not a valid message: {e}"),
            LineError::Version(v) => write!(f, "protocol version {v}, expected {PROTOCOL_VERSION}"),
        }
    }
}

impl std::error::Error for LineError {}

/// `msg` as one line including the `\n`. Fails when the line would exceed [`MAX_LINE`].
pub fn encode_line(msg: &Msg) -> Result<Vec<u8>, LineError> {
    let env = Envelope {
        v: PROTOCOL_VERSION,
        msg: msg.clone(),
    };
    let mut out = serde_json::to_vec(&env).map_err(|e| LineError::NotUtf8OrJson(e.to_string()))?;
    if out.len() > MAX_LINE {
        return Err(LineError::TooLong);
    }
    out.push(b'\n');
    Ok(out)
}

/// One received line (with or without its `\r\n`) as a message.
pub fn decode_line(line: &[u8]) -> Result<Msg, LineError> {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.len() > MAX_LINE {
        return Err(LineError::TooLong);
    }
    let env: Envelope =
        serde_json::from_slice(line).map_err(|e| LineError::NotUtf8OrJson(e.to_string()))?;
    if env.v != PROTOCOL_VERSION {
        return Err(LineError::Version(env.v));
    }
    Ok(env.msg)
}

/// What [`read_line`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Read1 {
    /// The peer closed the connection before any byte of a new line.
    Eof,
    /// A line (newline included when it was there).
    Line(Vec<u8>),
}

/// Reads one line, never buffering more than [`MAX_LINE`] bytes plus the newline: a peer that
/// sends endless data without a newline gets [`LineError::TooLong`] after 64 KiB, not memory.
pub fn read_line<R: BufRead>(r: &mut R) -> io::Result<Result<Read1, LineError>> {
    let mut buf = Vec::new();
    let n = r
        .by_ref()
        .take(MAX_LINE as u64 + 2)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(Ok(Read1::Eof));
    }
    let body = buf.strip_suffix(b"\n").unwrap_or(&buf);
    let body = body.strip_suffix(b"\r").unwrap_or(body);
    if body.len() > MAX_LINE {
        return Ok(Err(LineError::TooLong));
    }
    Ok(Ok(Read1::Line(buf)))
}

/// `\\.\pipe\phoecord-<user SID>`: one pipe per user, so two users on one machine never meet.
pub fn pipe_name(user_sid: &str) -> String {
    format!(r"\\.\pipe\phoecord-{user_sid}")
}

/// Waits (ms) before each connection attempt of the loader; after the last one it gives up. The
/// proxy usually runs already (the client is started by `phoecord launch`), and when it starts a
/// moment later the later attempts catch it.
pub const CONNECT_DELAYS_MS: [u64; 5] = [0, 500, 2_000, 5_000, 15_000];

/// What the proxy logs for a connected loader.
pub fn describe_connection(hello: &Msg, status: Option<&Msg>) -> Option<String> {
    let Msg::Hello {
        loader_version,
        client_version,
        text_hash,
        pid,
        detect_ms,
    } = hello
    else {
        return None;
    };
    let ui = match status {
        Some(Msg::Status { ui: true, .. }) => "UI layer on".to_string(),
        Some(Msg::Status {
            ui: false,
            reason: Some(r),
        }) => format!("UI layer off ({r})"),
        Some(Msg::Status {
            ui: false,
            reason: None,
        }) => "UI layer off".to_string(),
        _ => "UI layer status not reported".to_string(),
    };
    Some(format!(
        "loader connected: build {}, hash {text_hash}, {ui} (loader {loader_version}, pid {pid}, detected in {detect_ms} ms)",
        client_version.as_deref().unwrap_or("unknown"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, description_len: usize) -> PluginInfo {
        PluginInfo {
            id: id.into(),
            name: id.into(),
            version: "1.0.0".into(),
            description: "d".repeat(description_len),
            authors: vec![],
            package: false,
            crashed: false,
            rights: vec![],
            settings: vec![SettingInfo {
                key: "on".into(),
                kind: "bool".into(),
                default: Value::Bool(true),
                options: vec![],
                label: "On".into(),
            }],
        }
    }

    #[test]
    fn the_plugin_list_is_split_into_lines_that_fit() {
        let plugins: Vec<_> = (0..40).map(|i| info(&format!("p{i}"), 4000)).collect();
        let (lines, dropped) = plugins_lines(9, plugins.clone());
        assert!(dropped.is_empty());
        assert!(lines.len() > 1, "160 KB do not fit one line");
        let mut back = Vec::new();
        for (i, m) in lines.iter().enumerate() {
            let encoded = encode_line(m).unwrap();
            assert!(encoded.len() <= MAX_LINE + 1);
            match decode_line(&encoded).unwrap() {
                Msg::PluginsList { id, plugins, more } => {
                    assert_eq!(id, 9);
                    assert_eq!(more, i + 1 < lines.len());
                    back.extend(plugins);
                }
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(back, plugins, "all, in order");

        let (lines, dropped) = plugins_lines(1, vec![info("huge", MAX_LINE), info("ok", 10)]);
        assert_eq!(dropped, ["huge"]);
        assert_eq!(
            lines,
            [Msg::PluginsList {
                id: 1,
                plugins: vec![info("ok", 10)],
                more: false
            }]
        );
        assert_eq!(
            plugins_lines(2, vec![]).0,
            [Msg::PluginsList {
                id: 2,
                plugins: vec![],
                more: false
            }],
            "an empty list is still an answer"
        );
    }
    use serde_json::json;
    use std::io::Cursor;

    fn samples() -> Vec<Msg> {
        vec![
            Msg::Hello {
                loader_version: "0.5.0".into(),
                client_version: Some("1.0.1355".into()),
                text_hash: "ab12".into(),
                pid: 4242,
                detect_ms: 9,
            },
            Msg::Hello {
                loader_version: "0.5.0".into(),
                client_version: None,
                text_hash: "ab12".into(),
                pid: 1,
                detect_ms: 0,
            },
            Msg::Status {
                ui: false,
                reason: Some("no signatures".into()),
            },
            Msg::Status {
                ui: true,
                reason: None,
            },
            Msg::SettingsGet {
                id: 1,
                key: "ui".into(),
            },
            Msg::SettingsSet {
                id: 2,
                key: "ui".into(),
                value: json!(true),
            },
            Msg::SettingsValue {
                id: 2,
                key: "ui".into(),
                value: Some(json!("x")),
                error: None,
            },
            Msg::UiEvent {
                name: "settings.opened".into(),
                payload: json!({"tab": 3, "list": [1, 2]}),
            },
            menu_action("view_raw", "view_raw", &message_click(11, 50)),
            Msg::MenuGet { id: 7 },
            Msg::MenuItems {
                id: 7,
                items: vec![MenuItem {
                    plugin: "view_raw".into(),
                    entry: "view_raw".into(),
                    label: "Исходник".into(),
                    menu: "message".into(),
                }],
            },
        ]
    }

    fn message_click(channel: u64, message: u64) -> Click {
        Click {
            menu: "message".into(),
            channel_id: Some(channel),
            message_id: Some(message),
            ..Click::default()
        }
    }

    #[test]
    fn a_menu_action_reads_back() {
        let click = message_click(1219803441186996275, 7);
        let Msg::UiEvent { name, payload } = menu_action("p", "e", &click) else {
            panic!("not a ui.event");
        };
        assert_eq!(name, "menu_action");
        assert_eq!(payload["channel_id"], "1219803441186996275");
        assert!(payload.get("user_id").is_none(), "unset ids are left out");
        assert_eq!(
            parse_menu_action(&payload),
            Some(("p".into(), "e".into(), click))
        );
        // From before `menu` existed: a message-menu click.
        let old = json!({"plugin": "p", "entry": "e", "channel_id": "1", "message_id": "2"});
        assert_eq!(parse_menu_action(&old).unwrap().2, message_click(1, 2));
        assert_eq!(parse_menu_action(&json!({"plugin": "p"})), None);
        let user = Click {
            menu: "user".into(),
            user_id: Some(5),
            guild_id: Some(6),
            ..Click::default()
        };
        let Msg::UiEvent { payload, .. } = menu_action("p", "e", &user) else {
            panic!()
        };
        assert_eq!(parse_menu_action(&payload).unwrap().2, user);
        let item: MenuItem =
            serde_json::from_value(json!({"plugin": "p", "entry": "e", "label": "l"})).unwrap();
        assert_eq!(item.menu, "message");
    }

    #[test]
    fn every_message_round_trips_as_one_line() {
        for m in samples() {
            let line = encode_line(&m).unwrap();
            assert_eq!(line.iter().filter(|b| **b == b'\n').count(), 1);
            assert!(line.ends_with(b"\n"));
            assert_eq!(decode_line(&line).unwrap(), m);
        }
    }

    #[test]
    fn the_wire_format_is_the_documented_one() {
        let line = encode_line(&Msg::Status {
            ui: false,
            reason: Some("no signatures".into()),
        })
        .unwrap();
        let v: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(
            v,
            json!({"v": 1, "type": "status", "ui": false, "reason": "no signatures"})
        );
        let get = decode_line(br#"{"v":1,"type":"settings.get","id":7,"key":"k"}"#).unwrap();
        assert_eq!(
            get,
            Msg::SettingsGet {
                id: 7,
                key: "k".into()
            }
        );
        // CRLF is accepted.
        assert!(decode_line(b"{\"v\":1,\"type\":\"status\",\"ui\":true}\r\n").is_ok());
    }

    #[test]
    fn garbage_is_refused() {
        assert!(decode_line(b"hello").is_err());
        assert!(decode_line(b"").is_err());
        assert!(decode_line(&[0xff, 0xfe, b'\n']).is_err());
        assert!(decode_line(br#"{"v":1,"type":"nope"}"#).is_err());
        assert!(decode_line(br#"{"type":"status","ui":true}"#).is_err());
        assert_eq!(
            decode_line(br#"{"v":2,"type":"status","ui":true}"#),
            Err(LineError::Version(2))
        );
    }

    #[test]
    fn a_line_over_the_cap_is_refused_on_both_sides() {
        let big = Msg::UiEvent {
            name: "x".into(),
            payload: json!("a".repeat(MAX_LINE)),
        };
        assert_eq!(encode_line(&big), Err(LineError::TooLong));
        let mut long = vec![b'a'; MAX_LINE + 1];
        assert_eq!(decode_line(&long), Err(LineError::TooLong));
        // A line of exactly the cap passes the length check (and fails as JSON).
        long.truncate(MAX_LINE);
        assert!(matches!(
            decode_line(&long),
            Err(LineError::NotUtf8OrJson(_))
        ));
    }

    #[test]
    fn the_reader_stops_at_the_cap_without_buffering_the_rest() {
        let mut endless = Cursor::new(vec![b'x'; MAX_LINE * 4]);
        assert_eq!(read_line(&mut endless).unwrap(), Err(LineError::TooLong));
        assert!(
            endless.position() <= MAX_LINE as u64 + 2,
            "read {} bytes",
            endless.position()
        );

        let ok = format!("{}\n", "a".repeat(MAX_LINE));
        let mut c = Cursor::new(ok.into_bytes());
        assert!(
            matches!(read_line(&mut c).unwrap(), Ok(Read1::Line(l)) if l.len() == MAX_LINE + 1)
        );

        let mut two = Cursor::new(b"one\ntwo".to_vec());
        assert_eq!(
            read_line(&mut two).unwrap(),
            Ok(Read1::Line(b"one\n".to_vec()))
        );
        assert_eq!(
            read_line(&mut two).unwrap(),
            Ok(Read1::Line(b"two".to_vec()))
        );
        assert_eq!(read_line(&mut two).unwrap(), Ok(Read1::Eof));
    }

    #[test]
    fn the_pipe_name_has_the_sid_and_the_log_line_says_what_the_task_asks() {
        assert_eq!(pipe_name("S-1-5-21-1"), r"\\.\pipe\phoecord-S-1-5-21-1");
        let all = samples();
        let (hello, status) = (&all[0], &all[2]);
        assert_eq!(
            describe_connection(hello, Some(status)).unwrap(),
            "loader connected: build 1.0.1355, hash ab12, UI layer off (no signatures) (loader 0.5.0, pid 4242, detected in 9 ms)"
        );
        assert!(
            describe_connection(hello, None)
                .unwrap()
                .contains("UI layer status not reported")
        );
        assert_eq!(describe_connection(status, None), None);
    }
}
