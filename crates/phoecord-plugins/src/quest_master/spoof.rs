//! GAME and STREAM without patching the client: what goes on the wire while the script's fake
//! game (`Patcher`) or fake stream (`getStreamerActiveStreamMetadata`) is in place.
//!
//! - GAME: the presence the client would send for the fake process (`rpc()`'s activity in a gateway
//!   op 3, real "Playing" entries replaced as `applyExclusive` does), the previous presence sent
//!   again when the last fake game goes (`restore()`), and desktop heartbeats
//!   `{application_id, terminal}`.
//! - STREAM: heartbeats with the stream key of your own stream in your current voice channel
//!   (`guild:{guild}:{channel}:{you}` or `call:{channel}:{you}`).
//!
//! When the desktop shape is refused (400/403/404), or you are not in voice for a STREAM quest,
//! the script's soft heartbeats (`call:{quest}:1`, `game:{app}`) take over.

use super::*;

/// Gateway op 3: presence update. Op 4: voice state update.
pub(super) const PRESENCE_UPDATE: i64 = 3;
pub(super) const VOICE_STATE_UPDATE: i64 = 4;

/// One fake running game (`Patcher.games`).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct FakeGame {
    pub(super) quest: String,
    pub(super) app_id: String,
    pub(super) name: String,
    pub(super) start: u64,
}

impl FakeGame {
    /// The activity `rpc()` dispatches for the game, as the gateway carries it.
    fn activity(&self) -> Val {
        obj(&[
            ("application_id", Val::Str(self.app_id.clone())),
            ("name", Val::Str(self.name.clone())),
            ("type", Val::Int(0)),
            ("timestamps", obj(&[("start", Val::Int(self.start as i64))])),
        ])
    }
}

/// The presence data (`d`) of an op 3 frame or bare payload.
fn presence_data(payload: &Val) -> &Val {
    payload.get("d").unwrap_or(payload)
}

fn is_game(a: &Val) -> bool {
    a.get("type").and_then(Val::as_i64) == Some(0)
}

impl Engine {
    /// `Patcher.add` + `applyExclusive`: the fake game joins the presence.
    pub(super) fn fake_game_add(&mut self, host: &mut dyn Host, game: FakeGame) {
        if self.fake_games.iter().any(|g| g.quest == game.quest) {
            return;
        }
        self.fake_games.push(game);
        self.send_presence(host);
    }

    /// `Patcher.remove`: the game goes; the last one restores the previous presence.
    pub(super) fn fake_game_remove(&mut self, host: &mut dyn Host, quest: &str) {
        let before = self.fake_games.len();
        self.fake_games.retain(|g| g.quest != quest);
        if self.fake_games.len() != before {
            self.send_presence(host);
        }
    }

    /// `Patcher.clean`.
    pub(super) fn fake_games_clear(&mut self, host: &mut dyn Host) {
        if !self.fake_games.is_empty() {
            self.fake_games.clear();
            self.send_presence(host);
        }
    }

    /// The presence with the fake games (or, with none left, the real one), sent at once.
    /// `hideActivity` keeps the fake game out of the presence (the script skips `rpc()`).
    fn send_presence(&mut self, host: &mut dyn Host) {
        if flag(host, "hideActivity", false) {
            return;
        }
        let mut d = self.presence.clone().unwrap_or_else(|| {
            obj(&[
                ("since", Val::Int(0)),
                ("activities", Val::List(Vec::new())),
                ("status", Val::Str("online".into())),
                ("afk", Val::Bool(false)),
            ])
        });
        self.apply_fakes(&mut d);
        host.emit_send(PRESENCE_UPDATE, d);
    }

    /// Real "Playing" entries out, the fake games in (nothing changes without fake games).
    fn apply_fakes(&self, d: &mut Val) {
        if self.fake_games.is_empty() {
            return;
        }
        let mut list: Vec<Val> = d
            .get("activities")
            .map(Val::as_list)
            .unwrap_or(&[])
            .iter()
            .filter(|a| !is_game(a))
            .cloned()
            .collect();
        list.extend(self.fake_games.iter().map(FakeGame::activity));
        d.set("activities", Val::List(list));
    }

    /// Whether a presence already carries every fake game (then it is ours).
    fn has_fakes(&self, d: &Val) -> bool {
        let list = d.get("activities").map(Val::as_list).unwrap_or(&[]);
        self.fake_games.iter().all(|g| {
            list.iter()
                .any(|a| is_game(a) && a.str_at(&["application_id"]) == Some(g.app_id.as_str()))
        })
    }

    /// The client's own op 3: remember it (for `restore()`) and keep the fake games in it.
    pub(super) fn presence_sent(&mut self, host: &dyn Host, payload: &mut Val) -> bool {
        let d = presence_data(payload);
        if !matches!(d, Val::Map(_)) {
            return false;
        }
        if self.fake_games.is_empty() || !self.has_fakes(d) {
            self.presence = Some(d.clone());
        }
        if self.fake_games.is_empty() || flag(host, "hideActivity", false) || self.has_fakes(d) {
            return false;
        }
        let d = match payload.get("d") {
            Some(_) => payload.get_mut("d").expect("checked above"),
            None => payload,
        };
        self.apply_fakes(d);
        true
    }

    /// Your own voice channel, from your op 4 sends and your `VOICE_STATE_UPDATE`s.
    pub(super) fn voice_seen(&mut self, d: &Val) {
        let channel = d.id_at(&["channel_id"]);
        self.voice = channel.map(|c| (d.id_at(&["guild_id"]), c));
    }

    /// The stream key the client builds for your stream in your current voice channel.
    pub(super) fn stream_key(&self, host: &dyn Host) -> Option<String> {
        let me = host.self_id()?;
        let (guild, channel) = self.voice?;
        Some(match guild {
            Some(g) => format!("guild:{g}:{channel}:{me}"),
            None => format!("call:{channel}:{me}"),
        })
    }

    /// The heartbeat body for a GAME / STREAM step: the client's shape, or the script's soft key.
    pub(super) fn generic_beat_body(&self, host: &dyn Host, r: &mut Run, terminal: bool) -> Val {
        r.beat_primary = false;
        if !r.primary_rejected {
            match r.info.kind {
                Kind::Game => {
                    r.beat_primary = true;
                    return obj(&[
                        ("application_id", Val::Str(r.info.app_id.clone())),
                        ("terminal", Val::Bool(terminal)),
                    ]);
                }
                Kind::Stream => {
                    if let Some(key) = self.stream_key(host) {
                        r.beat_primary = true;
                        return obj(&[
                            ("stream_key", Val::Str(key)),
                            ("terminal", Val::Bool(terminal)),
                        ]);
                    }
                }
                _ => {}
            }
        }
        let sk = if terminal {
            r.soft_keys[0].clone()
        } else {
            r.soft_keys[r.key_idx % r.soft_keys.len()].clone()
        };
        obj(&[
            ("stream_key", Val::Str(sk)),
            ("terminal", Val::Bool(terminal)),
        ])
    }
}
