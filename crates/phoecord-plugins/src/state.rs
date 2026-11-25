//! Small gateway-state trackers shared by the data parts of UI plugins (stage 1.2.16 / 1.2.18).
//!
//! The Plugin API does not hand plugins the client's caches, so a plugin that needs to know
//! "which guilds and channels exist", "who sits in which voice channel" or "what a user is called"
//! keeps its own copy, fed from the dispatches it already sees. Each tracker here is opt-in: a
//! plugin embeds only the ones it needs, asks for the dispatches in [`Guilds::EVENTS`] and friends
//! from `Plugin::wants`, and calls `observe(name, payload)` from `on_event`. Trackers keep IDs,
//! names and counts only: no message content, no member lists.
//!
//! The trackers are plain structs; a plugin that shares one with its `/phoecord` command wraps it
//! in `Arc<Mutex<_>>` (see [`Shared`]).

use phoecord_core::Val;
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

/// A tracker shared between a plugin and its command. Never poisons the plugin: a panic in
/// another holder only loses that update.
pub struct Shared<T>(Arc<Mutex<T>>);

impl<T> Shared<T> {
    pub fn new(value: T) -> Shared<T> {
        Shared(Arc::new(Mutex::new(value)))
    }
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Self {
        Shared(self.0.clone())
    }
}

impl<T: Default> Default for Shared<T> {
    fn default() -> Self {
        Shared::new(T::default())
    }
}

/// `true` if `name` is one of `events`; for `Plugin::wants`.
pub fn is_one_of(name: &str, events: &[&str]) -> bool {
    events.contains(&name)
}

/// One guild channel as the trackers keep it.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub id: u64,
    pub guild_id: u64,
    pub name: String,
    /// Discord channel type (0 text, 2 voice, 4 category, 5 announcement, 13 stage, 15 forum…).
    pub kind: i64,
    pub parent_id: Option<u64>,
}

/// One guild as the trackers keep it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guild {
    pub id: u64,
    pub name: String,
    /// `member_count` from `GUILD_CREATE` / `READY`, kept current by member add/remove.
    pub member_count: Option<i64>,
    pub channels: BTreeSet<u64>,
}

/// Guilds and their channels.
#[derive(Debug, Default)]
pub struct Guilds {
    guilds: HashMap<u64, Guild>,
    channels: HashMap<u64, Channel>,
}

impl Guilds {
    /// Dispatches [`Guilds::observe`] reads.
    pub const EVENTS: &'static [&'static str] = &[
        "READY",
        "GUILD_CREATE",
        "GUILD_UPDATE",
        "GUILD_DELETE",
        "CHANNEL_CREATE",
        "CHANNEL_UPDATE",
        "CHANNEL_DELETE",
        "GUILD_MEMBER_ADD",
        "GUILD_MEMBER_REMOVE",
    ];

    pub fn observe(&mut self, name: &str, d: &Val) {
        match name {
            "READY" => {
                for g in d.get("guilds").map(Val::as_list).unwrap_or(&[]) {
                    self.add_guild(g);
                }
            }
            "GUILD_CREATE" => self.add_guild(d),
            "GUILD_UPDATE" => {
                if let Some(id) = d.id_at(&["id"]) {
                    let g = self.guilds.entry(id).or_insert_with(|| Guild {
                        id,
                        ..Guild::default()
                    });
                    if let Some(n) = guild_name(d) {
                        g.name = n.to_string();
                    }
                    if let Some(c) = d.get("member_count").and_then(Val::as_i64) {
                        g.member_count = Some(c);
                    }
                }
            }
            "GUILD_DELETE" => {
                // `unavailable: true` is an outage, not a leave: keep what we know.
                if d.get("unavailable").and_then(Val::as_bool) == Some(true) {
                    return;
                }
                if let Some(g) = d.id_at(&["id"]).and_then(|id| self.guilds.remove(&id)) {
                    for c in g.channels {
                        self.channels.remove(&c);
                    }
                }
            }
            "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
                if let Some(gid) = d.id_at(&["guild_id"]) {
                    self.add_channel(gid, d);
                }
            }
            "CHANNEL_DELETE" => {
                if let Some(c) = d.id_at(&["id"]).and_then(|id| self.channels.remove(&id))
                    && let Some(g) = self.guilds.get_mut(&c.guild_id)
                {
                    g.channels.remove(&c.id);
                }
            }
            "GUILD_MEMBER_ADD" | "GUILD_MEMBER_REMOVE" => {
                let delta = if name == "GUILD_MEMBER_ADD" { 1 } else { -1 };
                if let Some(g) = d
                    .id_at(&["guild_id"])
                    .and_then(|id| self.guilds.get_mut(&id))
                    && let Some(c) = g.member_count.as_mut()
                {
                    *c = (*c + delta).max(0);
                }
            }
            _ => {}
        }
    }

    fn add_guild(&mut self, d: &Val) {
        let Some(id) = d.id_at(&["id"]) else { return };
        if d.get("unavailable").and_then(Val::as_bool) == Some(true) && guild_name(d).is_none() {
            return;
        }
        let g = self.guilds.entry(id).or_insert_with(|| Guild {
            id,
            ..Guild::default()
        });
        if let Some(n) = guild_name(d) {
            g.name = n.to_string();
        }
        if let Some(c) = d.get("member_count").and_then(Val::as_i64) {
            g.member_count = Some(c);
        }
        for list in ["channels", "threads"] {
            for c in d.get(list).map(Val::as_list).unwrap_or(&[]) {
                self.add_channel(id, c);
            }
        }
    }

    fn add_channel(&mut self, guild_id: u64, c: &Val) {
        let Some(id) = c.id_at(&["id"]) else { return };
        let ch = Channel {
            id,
            guild_id,
            name: c.str_at(&["name"]).unwrap_or_default().to_string(),
            kind: c.get("type").and_then(Val::as_i64).unwrap_or(0),
            parent_id: c.id_at(&["parent_id"]),
        };
        self.guilds
            .entry(guild_id)
            .or_insert_with(|| Guild {
                id: guild_id,
                ..Guild::default()
            })
            .channels
            .insert(id);
        self.channels.insert(id, ch);
    }

    pub fn guild(&self, id: u64) -> Option<&Guild> {
        self.guilds.get(&id)
    }
    pub fn channel(&self, id: u64) -> Option<&Channel> {
        self.channels.get(&id)
    }
    /// All guilds, sorted by name.
    pub fn guilds(&self) -> Vec<&Guild> {
        let mut v: Vec<_> = self.guilds.values().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        v
    }
    /// A guild's channels, sorted by name.
    pub fn channels_of(&self, guild_id: u64) -> Vec<&Channel> {
        let mut v: Vec<_> = self
            .guild(guild_id)
            .map(|g| {
                g.channels
                    .iter()
                    .filter_map(|c| self.channels.get(c))
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        v
    }
}

/// User accounts send `properties.name` in `READY`, bots and older payloads `name`.
fn guild_name(d: &Val) -> Option<&str> {
    d.str_at(&["properties", "name"])
        .or_else(|| d.str_at(&["name"]))
}

/// Who is in which voice channel.
#[derive(Debug, Default)]
pub struct VoiceStates {
    /// user id -> (guild id or 0 for DM calls, channel id)
    users: HashMap<u64, (u64, u64)>,
}

/// A change [`VoiceStates::observe`] saw for one user.
#[derive(Clone, Debug, PartialEq)]
pub enum VoiceChange {
    Joined { user: u64, channel: u64 },
    Left { user: u64, channel: u64 },
    Moved { user: u64, from: u64, to: u64 },
}

impl VoiceStates {
    pub const EVENTS: &'static [&'static str] = &["READY", "GUILD_CREATE", "VOICE_STATE_UPDATE"];

    /// Updates the table; returns the change for `VOICE_STATE_UPDATE`.
    pub fn observe(&mut self, name: &str, d: &Val) -> Option<VoiceChange> {
        match name {
            "READY" => {
                for g in d.get("guilds").map(Val::as_list).unwrap_or(&[]) {
                    self.load_guild(g);
                }
                None
            }
            "GUILD_CREATE" => {
                self.load_guild(d);
                None
            }
            "VOICE_STATE_UPDATE" => {
                let user = d.id_at(&["user_id"])?;
                let guild = d.id_at(&["guild_id"]).unwrap_or(0);
                let before = self.users.get(&user).map(|&(_, c)| c);
                match d.id_at(&["channel_id"]) {
                    Some(to) => {
                        self.users.insert(user, (guild, to));
                        match before {
                            None => Some(VoiceChange::Joined { user, channel: to }),
                            Some(from) if from != to => Some(VoiceChange::Moved { user, from, to }),
                            Some(_) => None,
                        }
                    }
                    None => {
                        self.users.remove(&user);
                        before.map(|channel| VoiceChange::Left { user, channel })
                    }
                }
            }
            _ => None,
        }
    }

    fn load_guild(&mut self, g: &Val) {
        let Some(gid) = g.id_at(&["id"]) else { return };
        for vs in g.get("voice_states").map(Val::as_list).unwrap_or(&[]) {
            if let (Some(u), Some(c)) = (vs.id_at(&["user_id"]), vs.id_at(&["channel_id"])) {
                self.users.insert(u, (gid, c));
            }
        }
    }

    /// The channel a user is in.
    pub fn channel_of(&self, user: u64) -> Option<u64> {
        self.users.get(&user).map(|&(_, c)| c)
    }
    /// Users in a channel, sorted.
    pub fn in_channel(&self, channel: u64) -> Vec<u64> {
        let mut v: Vec<_> = self
            .users
            .iter()
            .filter(|(_, (_, c))| *c == channel)
            .map(|(u, _)| *u)
            .collect();
        v.sort_unstable();
        v
    }
    /// Users in voice anywhere in a guild, sorted.
    pub fn in_guild(&self, guild: u64) -> Vec<u64> {
        let mut v: Vec<_> = self
            .users
            .iter()
            .filter(|(_, (g, _))| *g == guild)
            .map(|(u, _)| *u)
            .collect();
        v.sort_unstable();
        v
    }
}

/// User id -> name to show, learnt from whatever user objects pass by. Bounded.
#[derive(Debug)]
pub struct Users {
    names: HashMap<u64, String>,
    order: VecDeque<u64>,
    cap: usize,
}

impl Default for Users {
    fn default() -> Self {
        Users::with_capacity(5000)
    }
}

impl Users {
    pub const EVENTS: &'static [&'static str] = &[
        "READY",
        "MESSAGE_CREATE",
        "GUILD_MEMBER_ADD",
        "GUILD_MEMBER_UPDATE",
        "PRESENCE_UPDATE",
        "TYPING_START",
        "VOICE_STATE_UPDATE",
        "RELATIONSHIP_ADD",
    ];

    pub fn with_capacity(cap: usize) -> Users {
        Users {
            names: HashMap::new(),
            order: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub fn observe(&mut self, name: &str, d: &Val) {
        match name {
            "READY" => {
                if let Some(u) = d.get("user") {
                    self.learn(u);
                }
                for u in d.get("users").map(Val::as_list).unwrap_or(&[]) {
                    self.learn(u);
                }
                for r in d.get("relationships").map(Val::as_list).unwrap_or(&[]) {
                    if let Some(u) = r.get("user") {
                        self.learn(u);
                    }
                }
            }
            "MESSAGE_CREATE" => {
                if let Some(u) = d.get("author") {
                    self.learn(u);
                }
                for u in d.get("mentions").map(Val::as_list).unwrap_or(&[]) {
                    self.learn(u);
                }
            }
            "TYPING_START" | "VOICE_STATE_UPDATE" => {
                if let Some(u) = d.at(&["member", "user"]) {
                    self.learn(u);
                }
            }
            _ => {
                if let Some(u) = d.get("user") {
                    self.learn(u);
                }
            }
        }
    }

    /// Learn one user object (`id`, `global_name`, `username`).
    pub fn learn(&mut self, u: &Val) {
        let Some(id) = u.id_at(&["id"]) else { return };
        let Some(name) = u
            .str_at(&["global_name"])
            .filter(|s| !s.is_empty())
            .or_else(|| u.str_at(&["username"]))
        else {
            return;
        };
        if self.names.insert(id, name.to_string()).is_none() {
            self.order.push_back(id);
            while self.order.len() > self.cap {
                if let Some(old) = self.order.pop_front() {
                    self.names.remove(&old);
                }
            }
        }
    }

    pub fn name(&self, id: u64) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }
    /// The name, or `<@id>` (a mention the client renders) when unknown.
    pub fn display(&self, id: u64) -> String {
        self.name(id)
            .map(str::to_string)
            .unwrap_or_else(|| format!("<@{id}>"))
    }
}

/// A bounded first-in first-out log, newest last.
#[derive(Debug)]
pub struct Ring<T> {
    items: VecDeque<T>,
    cap: usize,
}

impl<T> Ring<T> {
    pub fn new(cap: usize) -> Ring<T> {
        Ring {
            items: VecDeque::new(),
            cap: cap.max(1),
        }
    }
    pub fn push(&mut self, item: T) {
        self.items.push_back(item);
        while self.items.len() > self.cap {
            self.items.pop_front();
        }
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.items.iter()
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::j;

    #[test]
    fn guilds_follow_ready_create_update_delete() {
        let mut g = Guilds::default();
        g.observe(
            "READY",
            &j(r#"{"guilds":[{"id":"1","properties":{"name":"Alpha"},"member_count":10,
                "channels":[{"id":"11","name":"general","type":0},{"id":"12","name":"Voice","type":2,"parent_id":"10"}]},
                {"id":"2","unavailable":true}]}"#),
        );
        assert_eq!(g.guild(1).unwrap().name, "Alpha");
        assert!(g.guild(2).is_none());
        assert_eq!(g.channel(12).unwrap().parent_id, Some(10));
        g.observe(
            "GUILD_CREATE",
            &j(r#"{"id":"2","name":"Beta","channels":[]}"#),
        );
        assert_eq!(
            g.guilds()
                .iter()
                .map(|x| x.name.as_str())
                .collect::<Vec<_>>(),
            ["Alpha", "Beta"]
        );
        g.observe(
            "GUILD_MEMBER_ADD",
            &j(r#"{"guild_id":"1","user":{"id":"5"}}"#),
        );
        assert_eq!(g.guild(1).unwrap().member_count, Some(11));
        g.observe("CHANNEL_DELETE", &j(r#"{"id":"11","guild_id":"1"}"#));
        assert_eq!(g.channels_of(1).len(), 1);
        g.observe("GUILD_DELETE", &j(r#"{"id":"1","unavailable":true}"#));
        assert!(g.guild(1).is_some(), "an outage is not a leave");
        g.observe("GUILD_DELETE", &j(r#"{"id":"1"}"#));
        assert!(g.guild(1).is_none() && g.channel(12).is_none());
    }

    #[test]
    fn voice_states_report_join_move_leave() {
        let mut v = VoiceStates::default();
        v.observe(
            "GUILD_CREATE",
            &j(r#"{"id":"1","voice_states":[{"user_id":"7","channel_id":"12"}]}"#),
        );
        assert_eq!(v.in_channel(12), [7]);
        let join = j(r#"{"guild_id":"1","user_id":"8","channel_id":"12"}"#);
        assert_eq!(
            v.observe("VOICE_STATE_UPDATE", &join),
            Some(VoiceChange::Joined {
                user: 8,
                channel: 12
            })
        );
        assert_eq!(
            v.observe("VOICE_STATE_UPDATE", &join),
            None,
            "mute/deafen is no change"
        );
        assert_eq!(
            v.observe(
                "VOICE_STATE_UPDATE",
                &j(r#"{"guild_id":"1","user_id":"8","channel_id":"13"}"#)
            ),
            Some(VoiceChange::Moved {
                user: 8,
                from: 12,
                to: 13
            })
        );
        assert_eq!(
            v.observe(
                "VOICE_STATE_UPDATE",
                &j(r#"{"guild_id":"1","user_id":"8","channel_id":null}"#)
            ),
            Some(VoiceChange::Left {
                user: 8,
                channel: 13
            })
        );
        assert_eq!(v.in_guild(1), [7]);
    }

    #[test]
    fn users_prefer_global_name_and_stay_bounded() {
        let mut u = Users::with_capacity(2);
        u.observe(
            "MESSAGE_CREATE",
            &j(r#"{"author":{"id":"1","username":"a","global_name":"Ann"},"mentions":[{"id":"2","username":"bob","global_name":null}]}"#),
        );
        assert_eq!(u.name(1), Some("Ann"));
        assert_eq!(u.name(2), Some("bob"));
        u.learn(&j(r#"{"id":"3","username":"cid"}"#));
        assert_eq!(u.name(1), None, "oldest evicted");
        assert_eq!(u.display(9), "<@9>");
    }

    #[test]
    fn ring_keeps_the_newest() {
        let mut r = Ring::new(2);
        for i in 0..5 {
            r.push(i);
        }
        assert_eq!(r.iter().copied().collect::<Vec<_>>(), [3, 4]);
    }
}
