//! FakeDeafen: look muted and deafened to others while you still hear (Vencord userplugin: FakeDeafen).
//!
//! Ported from the owner's userplugin `FakeDeafen`
//! (https://github.com/Nanda070/vencord-plugins/tree/main/plugins/FakeDeafen, GPL-3.0-or-later per
//! the monorepo README, the files carry no header), modified for PhoeCord 2026-11-25.
//!
//! What the script does on the wire: while its switch is on, every outbound voice state update
//! (gateway op 4) is copied and gets `self_mute = true` (setting `fakeMute`) and `self_deaf = true`
//! (setting `fakeDeafen`) on top of whatever the client sent. Flipping the switch re-sends the
//! current voice state (guild, channel, real mute, deafen and video state, `flags: 0`) so the
//! change shows at once. Here the same rewrite is done on `Event::Send { op: 4 }`.
//!
//! Switching it on: `[plugins.fake_deafen] enabled = true` loads the plugin; the script's own
//! switch (the account-area button and Ctrl+Shift+Q) is `/phoecord fakedeafen [on|off]` (no
//! argument flips it) and starts off, like the script's `enabled = false`.
//!
//! Settings (`[plugins.fake_deafen]`, the script's names and defaults): `fakeMute` (`true`),
//! `fakeDeafen` (`true`).
//!
//! Not ported: the button and its icon, `showButton`, the keybind and `enableKeybind` (UI,
//! Phase 2). The refresh the script sends the moment the switch flips goes out with the next
//! heartbeat or voice dispatch instead: a `/phoecord` command cannot queue gateway sends.

use crate::commands::{Args, FnCmd, Subcommand};
use crate::state::Shared;
use phoecord_core::plugin::API_VERSION;
use phoecord_core::{Event, EventKind, Host, Layer, Manifest, Plugin, Val, Verdict};

const PLUGIN_ID: &str = "fake_deafen";
/// Gateway op 4: voice state update.
const VOICE_STATE_UPDATE: i64 = 4;
/// Gateway op 1: heartbeat. Carries the pending refresh out when nothing else does.
const HEARTBEAT: i64 = 1;

/// The last voice state the client itself sent (before any rewrite).
struct Real {
    guild_id: Val,
    channel_id: Val,
    self_mute: bool,
    self_deaf: bool,
    self_video: bool,
}

#[derive(Default)]
struct State {
    /// The script's `enabled`.
    enabled: bool,
    /// The switch was flipped and the refresh has not gone out yet.
    refresh: bool,
    last: Option<Real>,
}

pub struct FakeDeafen {
    manifest: Manifest,
    state: Shared<State>,
}

impl FakeDeafen {
    pub fn new() -> FakeDeafen {
        FakeDeafen {
            manifest: Manifest {
                id: PLUGIN_ID.into(),
                name: "FakeDeafen".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                description: "Fake deafen yourself".into(),
                authors: vec!["Cheterin Group".into()],
                vencord: Some("FakeDeafen".into()),
                layers: vec![Layer::Network],
                api: API_VERSION,
                fetch: Vec::new(),
            },
            state: Shared::default(),
        }
    }

    /// The `/phoecord fakedeafen` command, sharing this plugin's switch. Register it with
    /// `Commands::with`.
    pub fn command(&self) -> Box<dyn Subcommand> {
        let state = self.state.clone();
        FnCmd::new(
            PLUGIN_ID,
            "fakedeafen",
            "Look deafened to others while you still hear",
            move |host, args: &Args| {
                let mut st = state.lock();
                let want = match args
                    .get("state")
                    .map(|s| s.trim().to_ascii_lowercase())
                    .as_deref()
                {
                    None | Some("") => !st.enabled,
                    Some("on" | "true" | "1" | "yes") => true,
                    Some("off" | "false" | "0" | "no") => false,
                    Some(other) => {
                        return host.t("fake_deafen-unknown-state", &[("state", other)]);
                    }
                };
                st.enabled = want;
                st.refresh = true;
                host.t(
                    if want {
                        "fake_deafen-on"
                    } else {
                        "fake_deafen-off"
                    },
                    &[],
                )
            },
        )
        .option("state", "on or off; leave empty to flip", false)
        .boxed()
    }
}

impl Default for FakeDeafen {
    fn default() -> Self {
        Self::new()
    }
}

fn flag(host: &dyn Host, key: &str) -> bool {
    host.setting(key).and_then(|v| v.as_bool()) != Some(false)
}

fn bool_at(d: &Val, key: &str) -> bool {
    d.get(key).and_then(Val::as_bool) == Some(true)
}

/// The script's `hookedSend`: `self_mute` and `self_deaf` forced on. `true` if something changed.
fn fake(host: &dyn Host, d: &mut Val) -> bool {
    let mut changed = false;
    for (setting, field) in [("fakeMute", "self_mute"), ("fakeDeafen", "self_deaf")] {
        if flag(host, setting) && d.get(field) != Some(&Val::Bool(true)) {
            d.set(field, Val::Bool(true));
            changed = true;
        }
    }
    changed
}

impl State {
    fn remember(&mut self, d: &Val) {
        self.last = Some(Real {
            guild_id: d.get("guild_id").cloned().unwrap_or(Val::Null),
            channel_id: d.get("channel_id").cloned().unwrap_or(Val::Null),
            self_mute: bool_at(d, "self_mute"),
            self_deaf: bool_at(d, "self_deaf"),
            self_video: bool_at(d, "self_video"),
        });
    }

    /// The script's `refresh_voice_state`: nothing when not in a voice channel.
    fn refresh_payload(&self, host: &dyn Host) -> Option<Val> {
        let last = self.last.as_ref()?;
        if matches!(last.channel_id, Val::Null) {
            return None;
        }
        let guild = last
            .channel_id
            .snowflake()
            .and_then(|c| host.channel_guild(c))
            .map(|g| Val::Str(g.to_string()))
            .unwrap_or_else(|| last.guild_id.clone());
        let mut d = Val::map();
        d.set("guild_id", guild);
        d.set("channel_id", last.channel_id.clone());
        d.set("self_mute", Val::Bool(last.self_mute));
        d.set("self_deaf", Val::Bool(last.self_deaf));
        d.set("self_video", Val::Bool(last.self_video));
        d.set("flags", Val::Int(0));
        // Queued sends do not pass the Send event again, so the fake goes in here.
        if self.enabled {
            fake(host, &mut d);
        }
        Some(d)
    }

    fn flush(&mut self, host: &mut dyn Host) {
        if !self.refresh {
            return;
        }
        self.refresh = false;
        if let Some(d) = self.refresh_payload(&*host) {
            host.emit_send(VOICE_STATE_UPDATE, d);
            host.log(if self.enabled {
                "voice state updated to fake deafen"
            } else {
                "voice state updated to normal"
            });
        }
    }
}

impl Plugin for FakeDeafen {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn wants(&self, event: &EventKind) -> bool {
        match event {
            EventKind::Send { op } => *op == VOICE_STATE_UPDATE || *op == HEARTBEAT,
            EventKind::Dispatch { name } => *name == "VOICE_STATE_UPDATE",
            _ => false,
        }
    }

    fn wants_gateway(&self) -> bool {
        true
    }

    fn on_event(&self, host: &mut dyn Host, event: Event<'_>) -> Verdict {
        match event {
            Event::Send {
                op: VOICE_STATE_UPDATE,
                payload,
            } => {
                let Some(d) = payload.get_mut("d") else {
                    return Verdict::Pass;
                };
                if !matches!(d, Val::Map(_)) {
                    return Verdict::Pass;
                }
                let mut st = self.state.lock();
                st.remember(d);
                let changed = st.enabled && fake(&*host, d);
                st.flush(host);
                if changed {
                    Verdict::Changed
                } else {
                    Verdict::Pass
                }
            }
            Event::Send { op: HEARTBEAT, .. } | Event::Dispatch { .. } => {
                self.state.lock().flush(host);
                Verdict::Pass
            }
            _ => Verdict::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{Rec, j};
    use phoecord_plugin_runtime::{Chain, Outcome};

    fn setup() -> (Chain, Box<dyn Subcommand>) {
        let p = FakeDeafen::new();
        let cmd = p.command();
        (Chain::new(vec![Box::new(p)]), cmd)
    }

    fn args(state: Option<&str>) -> Args {
        Args {
            options: state
                .map(|s| vec![("state".to_string(), s.to_string())])
                .unwrap_or_default(),
        }
    }

    fn op4(chain: &Chain, h: &mut Rec, json: &str) -> (Outcome, Val) {
        let mut f = j(json);
        let out = chain.send(h, 4, &mut f);
        (out, f)
    }

    const REAL: &str = r#"{"op":4,"d":{"guild_id":"9","channel_id":"20","self_mute":false,"self_deaf":false,"self_video":false}}"#;

    #[test]
    fn off_by_default_and_nothing_changes() {
        let (chain, _cmd) = setup();
        let mut h = Rec::default();
        let (out, f) = op4(&chain, &mut h, REAL);
        assert_eq!(out, Outcome::Untouched);
        assert_eq!(f, j(REAL));
    }

    #[test]
    fn switched_on_it_forces_mute_and_deafen_on_the_wire() {
        let (chain, cmd) = setup();
        let mut h = Rec::default();
        assert!(cmd.run(&mut h, &args(Some("on"))).contains("on"));
        let (out, f) = op4(&chain, &mut h, REAL);
        assert_eq!(out, Outcome::Changed);
        assert_eq!(f.at(&["d", "self_mute"]), Some(&Val::Bool(true)));
        assert_eq!(f.at(&["d", "self_deaf"]), Some(&Val::Bool(true)));
        assert_eq!(f.str_at(&["d", "channel_id"]), Some("20"));
        // Leaving voice is rewritten too, like the script (any op 4 with data).
        let (out, f) = op4(
            &chain,
            &mut h,
            r#"{"op":4,"d":{"guild_id":"9","channel_id":null}}"#,
        );
        assert_eq!(out, Outcome::Changed);
        assert_eq!(f.at(&["d", "self_deaf"]), Some(&Val::Bool(true)));
    }

    #[test]
    fn settings_pick_what_is_faked() {
        let (chain, cmd) = setup();
        let mut h = Rec::default()
            .with("fakeMute", Val::Bool(false))
            .with("fakeDeafen", Val::Bool(true));
        cmd.run(&mut h, &args(Some("on")));
        let (_, f) = op4(&chain, &mut h, REAL);
        assert_eq!(f.at(&["d", "self_mute"]), Some(&Val::Bool(false)));
        assert_eq!(f.at(&["d", "self_deaf"]), Some(&Val::Bool(true)));
        let mut none = Rec::default()
            .with("fakeMute", Val::Bool(false))
            .with("fakeDeafen", Val::Bool(false));
        let (out, _) = op4(&chain, &mut none, REAL);
        assert_eq!(out, Outcome::Untouched);
    }

    #[test]
    fn flipping_the_switch_sends_the_real_state_with_the_fake_on_top() {
        let (chain, cmd) = setup();
        let mut h = Rec::default();
        // No voice channel known yet: nothing to refresh.
        cmd.run(&mut h, &args(Some("on")));
        chain.send(&mut h, 1, &mut j(r#"{"op":1,"d":5}"#));
        assert!(h.sent.is_empty());
        cmd.run(&mut h, &args(Some("off")));
        op4(&chain, &mut h, REAL);
        h.sent.clear();
        // Flip on: the next heartbeat carries the refresh, mute and deafen forced.
        assert!(cmd.run(&mut h, &args(None)).contains("on"));
        chain.send(&mut h, 1, &mut j(r#"{"op":1,"d":5}"#));
        assert_eq!(h.sent.len(), 1);
        let (op, d) = &h.sent[0];
        assert_eq!(*op, 4);
        assert_eq!(d.str_at(&["channel_id"]), Some("20"));
        assert_eq!(d.at(&["self_mute"]), Some(&Val::Bool(true)));
        assert_eq!(d.at(&["self_deaf"]), Some(&Val::Bool(true)));
        assert_eq!(d.at(&["flags"]), Some(&Val::Int(0)));
        // Once only; flip off sends the real state again.
        chain.send(&mut h, 1, &mut j(r#"{"op":1,"d":5}"#));
        assert_eq!(h.sent.len(), 1);
        cmd.run(&mut h, &args(None));
        chain.send(&mut h, 1, &mut j(r#"{"op":1,"d":6}"#));
        assert_eq!(h.sent[1].1.at(&["self_deaf"]), Some(&Val::Bool(false)));
    }

    #[test]
    fn malformed_frames_and_bad_state_do_not_panic() {
        let (chain, cmd) = setup();
        let mut h = Rec::default();
        cmd.run(&mut h, &args(Some("on")));
        for f in [r#"{"op":4}"#, r#"{"op":4,"d":null}"#, r#"{"op":4,"d":5}"#] {
            assert_eq!(op4(&chain, &mut h, f).0, Outcome::Untouched);
        }
        assert!(cmd.run(&mut h, &args(Some("maybe"))).contains("maybe"));
        h.plugins.insert(PLUGIN_ID.into(), false);
        assert!(cmd.run(&mut h, &args(None)).contains("is off"));
    }
}
