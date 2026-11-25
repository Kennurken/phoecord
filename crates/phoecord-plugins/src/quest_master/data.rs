//! QuestMaster's pure helpers: random numbers, ISO times, the script's text cleanup and
//! reading quest objects (QuestStore camelCase or API snake_case).

use phoecord_core::{Host, Val};
use std::time::{SystemTime, UNIX_EPOCH};

/* ── small helpers ─────────────────────────────────────────────── */

pub(super) fn system_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// xorshift64*: the script's `Math.random()`, seeded from the clock.
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn seeded() -> Rng {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0x2545_f491, |d| d.as_nanos() as u64);
        Rng((nanos ^ 0x9e37_79b9_7f4a_7c15) | 1)
    }
    pub(super) fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    /// `Math.random()`: `[0, 1)`.
    pub(super) fn random(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// The script's `rnd(min, max)`: an integer in `[min, max]`.
    pub(super) fn rnd(&mut self, min: u64, max: u64) -> u64 {
        (self.random() * (max - min + 1) as f64).floor() as u64 + min
    }
}

/// A JS `??` lookup: the key, unless it is missing or `null`.
pub(super) fn get<'a>(v: &'a Val, key: &str) -> Option<&'a Val> {
    v.get(key).filter(|x| !matches!(x, Val::Null))
}

/// The camelCase key (QuestStore shape) or the snake_case one (API shape).
pub(super) fn pick<'a>(v: &'a Val, camel: &str, snake: &str) -> Option<&'a Val> {
    get(v, camel).or_else(|| get(v, snake))
}

/// `typeof x === "number"`.
pub(super) fn num(v: &Val) -> Option<f64> {
    match v {
        Val::Int(n) => Some(*n as f64),
        Val::Big(n) => Some(*n as f64),
        Val::Float(f) => Some(*f),
        _ => None,
    }
}

/// A string or snowflake as text.
pub(super) fn text(v: &Val) -> Option<String> {
    match v {
        Val::Str(s) | Val::Atom(s) => Some(s.clone()),
        Val::Int(n) => Some(n.to_string()),
        Val::Big(n) => Some(n.to_string()),
        _ => None,
    }
}

/// A number the way JS prints it (`900`, not `900.0`).
pub(super) fn n(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

/// `Number(x) || fallback` for a slider setting.
pub(super) fn setting_num(host: &dyn Host, key: &str, default: f64) -> f64 {
    let v = host.setting(key).and_then(|v| match v {
        Val::Str(s) => s.trim().parse::<f64>().ok(),
        v => num(&v),
    });
    match v {
        Some(x) if x != 0.0 && !x.is_nan() => x,
        _ => default,
    }
}

pub(super) fn flag(host: &dyn Host, key: &str, default: bool) -> bool {
    host.setting(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

pub(super) fn obj(fields: &[(&str, Val)]) -> Val {
    let mut m = Val::map();
    for (k, v) in fields {
        m.set(k, v.clone());
    }
    m
}

/// Days since 1970-01-01 for a civil date.
pub(super) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `new Date(s).getTime()` for Discord's ISO 8601 times; `None` is JS `NaN`.
pub(super) fn parse_iso(s: &str) -> Option<i64> {
    let b = s.trim().as_bytes();
    let dig = |r: std::ops::Range<usize>| -> Option<i64> {
        let part = b.get(r)?;
        if part.iter().all(u8::is_ascii_digit) {
            std::str::from_utf8(part).ok()?.parse().ok()
        } else {
            None
        }
    };
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (y, mo, d) = (dig(0..4)?, dig(5..7)?, dig(8..10)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let mut ms = days_from_civil(y, mo, d) * 86_400_000;
    if b.len() == 10 {
        return Some(ms);
    }
    if !matches!(b[10], b'T' | b't' | b' ') || b.len() < 19 || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (h, mi, sec) = (dig(11..13)?, dig(14..16)?, dig(17..19)?);
    ms += ((h * 60 + mi) * 60 + sec) * 1000;
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        let frac = std::str::from_utf8(&b[start..i]).ok()?;
        let three: String = frac.chars().chain("000".chars()).take(3).collect();
        ms += three.parse::<i64>().ok()?;
    }
    match b.get(i) {
        None | Some(b'Z' | b'z') => Some(ms),
        Some(sign @ (b'+' | b'-')) => {
            let (oh, om) = (dig(i + 1..i + 3)?, dig(i + 4..i + 6)?);
            let off = (oh * 60 + om) * 60_000;
            Some(if *sign == b'+' { ms - off } else { ms + off })
        }
        _ => None,
    }
}

/// `new Date(ms).toISOString()`.
pub(super) fn fmt_iso(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        ms % 1000
    )
}

/// The script's `sanitizeErrText`.
pub(super) fn sanitize(raw: &str, max: usize) -> String {
    let lower = raw.trim_start().to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        return "[HTML error page]".into();
    }
    let s = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s
    }
}

/// `X` in milliseconds from the PhoeCord limiter's "... retry in X.Xs".
pub(super) fn retry_in_ms(msg: &str) -> Option<u64> {
    let rest = &msg[msg.find("retry in ")? + "retry in ".len()..];
    let secs: f64 = rest.split('s').next()?.trim().parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| (secs * 1000.0).round() as u64)
}

/* ── quest data (QuestStore objects or API objects) ────────────── */

pub(super) fn q_id(q: &Val) -> Option<String> {
    get(q, "id").and_then(text)
}
pub(super) fn q_status(q: &Val) -> Option<&Val> {
    pick(q, "userStatus", "user_status")
}
pub(super) fn q_enrolled(q: &Val) -> Option<&Val> {
    q_status(q).and_then(|s| pick(s, "enrolledAt", "enrolled_at"))
}
pub(super) fn q_completed(q: &Val) -> bool {
    q_status(q)
        .and_then(|s| pick(s, "completedAt", "completed_at"))
        .is_some()
}
pub(super) fn q_config(q: &Val) -> Option<&Val> {
    get(q, "config")
}
/// `config.taskConfig ?? config.taskConfigV2` (API: `task_config`, `task_config_v2`).
pub(super) fn q_task_cfg(q: &Val) -> Option<&Val> {
    let c = q_config(q)?;
    get(c, "taskConfig")
        .or_else(|| get(c, "taskConfigV2"))
        .or_else(|| get(c, "task_config"))
        .or_else(|| get(c, "task_config_v2"))
}
pub(super) fn q_app_id(q: &Val) -> Option<String> {
    q_config(q)
        .and_then(|c| get(c, "application"))
        .and_then(|a| get(a, "id"))
        .and_then(text)
}
pub(super) fn q_name(q: &Val) -> String {
    q_config(q)
        .and_then(|c| get(c, "messages"))
        .and_then(|m| pick(m, "questName", "quest_name"))
        .and_then(text)
        .unwrap_or_else(|| "Unknown Quest".into())
}
/// The first reward: `(type, label, text)` as the picker shows it.
pub(super) fn q_reward(q: &Val) -> (i64, &'static str, String) {
    let rw = q_config(q)
        .and_then(|c| pick(c, "rewardsConfig", "rewards_config"))
        .and_then(|r| get(r, "rewards"))
        .and_then(|l| l.as_list().first());
    let kind = rw
        .and_then(|r| get(r, "type"))
        .and_then(Val::as_i64)
        .unwrap_or(0);
    let label = match kind {
        1 => "IN-GAME",
        3 => "AVATAR DECORATION",
        4 => "ORBS",
        _ => "OTHER",
    };
    let name = rw
        .and_then(|r| get(r, "messages"))
        .and_then(|m| get(m, "name"))
        .and_then(text)
        .unwrap_or_else(|| "Unknown Reward".into());
    (kind, label, name)
}
/// The script's `notExpired`: a missing `expiresAt` is time 0 (expired), an unreadable one NaN.
pub(super) fn not_expired(q: &Val, now: u64) -> bool {
    let raw = q_config(q).and_then(|c| pick(c, "expiresAt", "expires_at"));
    let e = match raw {
        None => Some(0),
        Some(v) => match v {
            Val::Str(s) => parse_iso(s),
            v => num(v).map(|f| f as i64),
        },
    };
    e.is_none_or(|e| e > now as i64)
}
/// `status.progress[key].value` when it is a number.
pub(super) fn prog(status: Option<&Val>, key: &str) -> Option<f64> {
    status
        .and_then(|s| get(s, "progress"))
        .and_then(|p| get(p, key))
        .and_then(|k| get(k, "value"))
        .and_then(num)
}
/// `streamProgressSeconds` (API: `stream_progress_seconds`).
pub(super) fn stream_secs(status: Option<&Val>) -> Option<f64> {
    status
        .and_then(|s| pick(s, "streamProgressSeconds", "stream_progress_seconds"))
        .and_then(num)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Video,
    Game,
    Stream,
    Activity,
    Achievement,
}

impl Kind {
    /// What the picker and the cards show.
    pub(super) fn label(self) -> &'static str {
        match self {
            Kind::Video => "VIDEO",
            Kind::Game => "GAME",
            Kind::Stream => "STREAM",
            Kind::Activity => "ACTIVITY",
            Kind::Achievement => "ACHIEVEMENT",
        }
    }
}

/// `Tasks.detectType`: exact keys first, then fuzzy (never the short tokens), then GAME by app id.
pub(super) fn detect_type(cfg: &Val, app_id: Option<&str>) -> Option<(Kind, String, f64)> {
    const MAP: [(&str, Kind); 11] = [
        ("ACHIEVEMENT_IN_ACTIVITY", Kind::Achievement),
        ("WATCH_VIDEO_ON_MOBILE", Kind::Video),
        ("WATCH_VIDEO", Kind::Video),
        ("PLAY_ON_DESKTOP_V2", Kind::Game),
        ("STREAM_ON_DESKTOP", Kind::Stream),
        ("PLAY_ON_DESKTOP", Kind::Game),
        ("PLAY_ACTIVITY", Kind::Activity),
        ("STREAM", Kind::Stream),
        ("VIDEO", Kind::Video),
        ("ACTIVITY", Kind::Activity),
        ("PLAY", Kind::Game),
    ];
    let tasks = get(cfg, "tasks")?;
    let Val::Map(entries) = tasks else {
        return None;
    };
    let keys: Vec<String> = entries.iter().filter_map(|(k, _)| text(k)).collect();
    let target = |k: &str| {
        get(tasks, k)
            .and_then(|t| get(t, "target"))
            .and_then(num)
            .unwrap_or(0.0)
    };
    for (key, kind) in MAP {
        if keys.iter().any(|k| k == key) {
            return Some((kind, key.to_string(), target(key)));
        }
    }
    for (key, kind) in MAP {
        if matches!(key, "PLAY" | "STREAM" | "VIDEO" | "ACTIVITY") {
            continue;
        }
        if let Some(k) = keys.iter().find(|k| k.contains(key)) {
            return Some((kind, k.clone(), target(k)));
        }
    }
    if app_id.is_some_and(|a| !a.is_empty()) {
        let first = keys.first().cloned();
        let t = first.as_deref().map_or(0.0, target);
        return Some((
            Kind::Game,
            first.unwrap_or_else(|| "PLAY_ON_DESKTOP".into()),
            t,
        ));
    }
    None
}
