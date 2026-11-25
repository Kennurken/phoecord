//! QuestMaster tests: synthetic quest payloads only, no network.

use super::*;
use crate::testutil::{Rec, j};
use phoecord_plugin_runtime::Chain;

const NOW: u64 = 1_800_000_000_000;

fn plugin() -> (QuestMaster, Shared<Engine>) {
    let p = QuestMaster::new();
    let st = p.state.clone();
    st.lock().clock = Some(NOW);
    (p, st)
}

fn quest(id: &str, tasks: &str, enrolled: bool, extra_status: &str) -> String {
    let enrolled_at = if enrolled {
        r#""enrolled_at":"2027-01-15T07:00:00.000000+00:00","#
    } else {
        ""
    };
    format!(
        r#"{{"id":"{id}","config":{{"expires_at":"2030-01-01T00:00:00+00:00",
        "application":{{"id":"555","name":"Game"}},
        "messages":{{"quest_name":"Quest {id}"}},
        "rewards_config":{{"rewards":[{{"type":4,"messages":{{"name":"700 Orbs"}}}}]}},
        "task_config_v2":{{"tasks":{tasks}}}}},
        "user_status":{{{enrolled_at}"completed_at":null{extra_status}}}}}"#
    )
}

fn quests_body(qs: &[String]) -> String {
    format!(r#"{{"quests":[{}]}}"#, qs.join(","))
}

fn set_answer(h: &mut Rec, method: &str, route: &str, body: &str) {
    let r = std::mem::take(h).answer(method, route, 200, body);
    *h = r;
}

fn fire(p: &QuestMaster, h: &mut Rec, tag: &str) {
    let mut f = Rec::timer_frame(tag);
    p.on_event(
        h,
        Event::Dispatch {
            name: TIMER_EVENT,
            payload: &mut f,
            extra: &mut Vec::new(),
        },
    );
}

/// Run `/phoecord quests <action> [quest]` the way the chain does: this plugin sees the
/// interaction first, the command answers, then the `cmd` timer runs the work.
fn command(p: &QuestMaster, h: &mut Rec, action: &str, quest: Option<&str>) -> String {
    let mut body =
        j(r#"{"type":2,"data":{"name":"phoecord","options":[{"name":"quests","type":1}]}}"#);
    p.on_event(
        h,
        Event::RestRequest {
            method: "POST",
            route: "/interactions",
            body: Some(&mut body),
        },
    );
    let mut options = vec![("action".to_string(), action.to_string())];
    if let Some(q) = quest {
        options.push(("quest".into(), q.into()));
    }
    let text = p.command().run(h, &Args { options });
    fire(p, h, CMD_TAG);
    text
}

fn fire_last(p: &QuestMaster, h: &mut Rec, kind: &str) {
    let tag = last_tag(h, kind);
    fire(p, h, &tag);
}

/// The newest pending tag of a kind (`task`, `traffic`, ...).
fn last_tag(h: &Rec, kind: &str) -> String {
    h.scheduled
        .iter()
        .rev()
        .find(|(_, t)| t.split('|').nth(1) == Some(kind))
        .map(|(_, t)| t.clone())
        .unwrap()
}

fn calls(h: &Rec, prefix: &str) -> Vec<Option<Val>> {
    h.rest_calls
        .iter()
        .filter(|(k, _)| k.starts_with(prefix))
        .map(|(_, b)| b.clone())
        .collect()
}

#[test]
fn detects_every_quest_type_like_the_script() {
    let d =
        |tasks: &str, app: Option<&str>| detect_type(&j(&format!(r#"{{"tasks":{tasks}}}"#)), app);
    assert_eq!(
        d(r#"{"WATCH_VIDEO":{"target":900}}"#, None),
        Some((Kind::Video, "WATCH_VIDEO".into(), 900.0))
    );
    assert_eq!(
        d(r#"{"WATCH_VIDEO_ON_MOBILE":{"target":60}}"#, None)
            .unwrap()
            .0,
        Kind::Video
    );
    assert_eq!(
        d(r#"{"PLAY_ON_DESKTOP":{"target":900}}"#, None).unwrap().0,
        Kind::Game
    );
    assert_eq!(
        d(r#"{"STREAM_ON_DESKTOP":{"target":900}}"#, None)
            .unwrap()
            .0,
        Kind::Stream
    );
    // Exact keys first: PLAY never steals PLAY_ACTIVITY.
    assert_eq!(
        d(r#"{"PLAY_ACTIVITY":{"target":600}}"#, None).unwrap().0,
        Kind::Activity
    );
    assert_eq!(
        d(r#"{"ACHIEVEMENT_IN_ACTIVITY":{"target":1}}"#, None)
            .unwrap()
            .0,
        Kind::Achievement
    );
    // Fuzzy pass on the long keys.
    assert_eq!(
        d(r#"{"PLAY_ON_DESKTOP_XL":{"target":5}}"#, None),
        Some((Kind::Game, "PLAY_ON_DESKTOP_XL".into(), 5.0))
    );
    // Unknown key: GAME when the quest has an application, else nothing.
    assert_eq!(
        d(r#"{"SOMETHING":{"target":7}}"#, Some("1")),
        Some((Kind::Game, "SOMETHING".into(), 7.0))
    );
    assert_eq!(d(r#"{"SOMETHING":{"target":7}}"#, None), None);
    assert_eq!(d(r#"[]"#, Some("1")), None);
}

#[test]
fn video_quest_runs_enrolled_progress_steps_until_completed() {
    let (p, st) = plugin();
    let q = quest("1", r#"{"WATCH_VIDEO":{"target":10}}"#, true, "");
    let mut h = Rec::default()
        .answer("GET", "/quests/@me", 200, &quests_body(&[q]))
        .answer(
            "POST",
            "/quests/1/video-progress",
            200,
            r#"{"progress":{"WATCH_VIDEO":{"value":7}},"completed_at":null}"#,
        );
    assert_eq!(
        command(&p, &mut h, "start", None),
        h.t("quest_master-starting", &[])
    );
    // Opened, picker chosen, first cycle: GET twice, then the first progress step at once.
    assert_eq!(calls(&h, "GET /quests/@me").len(), 2);
    let steps = calls(&h, "POST /quests/1/video-progress");
    assert_eq!(steps.len(), 1);
    assert_eq!(
        steps[0].as_ref().unwrap().get("timestamp"),
        Some(&Val::Int(7))
    );
    assert_eq!(st.lock().cards["1"].cur, 7.0);
    // The queue waits 1.2–1.8 s; the task ticks after 1 s.
    let traffic = h
        .scheduled
        .iter()
        .find(|(_, t)| t.contains("|traffic|"))
        .unwrap()
        .0;
    assert!((1200..=1800).contains(&traffic));
    let task = last_tag(&h, "task");
    assert_eq!(
        h.scheduled.iter().find(|(_, t)| *t == task).unwrap().0,
        TICK_MS
    );
    // Next step is queued behind the pause, then sent when the queue turns.
    st.lock().clock = Some(NOW + 2000);
    fire(&p, &mut h, &task);
    assert_eq!(calls(&h, "POST /quests/1/video-progress").len(), 1);
    set_answer(
        &mut h,
        "POST",
        "/quests/1/video-progress",
        r#"{"progress":{"WATCH_VIDEO":{"value":10}},"completed_at":"2027-01-15T08:00:00+00:00"}"#,
    );
    fire_last(&p, &mut h, "traffic");
    let steps = calls(&h, "POST /quests/1/video-progress");
    assert_eq!(steps.len(), 2);
    assert_eq!(
        steps[1].as_ref().unwrap().get("timestamp"),
        Some(&Val::Int(10))
    );
    let s = st.lock();
    assert_eq!(s.cards["1"].status, Status::Completed);
    assert!(s.cards["1"].claimable);
    assert_eq!(h.notes.len(), 1);
    assert_eq!(h.notes[0].1, "Quest 1");
}

#[test]
fn video_never_runs_ahead_of_the_time_since_enrolment() {
    let (p, st) = plugin();
    // Enrolled 3 s ago: at most 3 + 10 seconds of video may be reported.
    let mut q = j(&quest(
        "1",
        r#"{"WATCH_VIDEO":{"target":100}}"#,
        true,
        r#","progress":{"WATCH_VIDEO":{"value":13}}"#,
    ));
    q.get_mut("user_status")
        .unwrap()
        .set("enrolled_at", Val::Str(fmt_iso(NOW - 3000)));
    let body = format!(r#"{{"quests":[{}]}}"#, q.to_json());
    let mut h = Rec::default().answer("GET", "/quests/@me", 200, &body);
    command(&p, &mut h, "start", None);
    assert!(calls(&h, "POST /quests/1/video-progress").is_empty());
    let task = last_tag(&h, "task");
    assert_eq!(
        h.scheduled.iter().find(|(_, t)| *t == task).unwrap().0,
        TICK_MS
    );
    assert_eq!(st.lock().cards["1"].cur, 13.0);
}

#[test]
fn unenrolled_quest_is_enrolled_first_and_skipped_when_gone() {
    let (p, st) = plugin();
    let q = quest("2", r#"{"WATCH_VIDEO":{"target":30}}"#, false, "");
    let mut h = Rec::default().answer("GET", "/quests/@me", 200, &quests_body(&[q]));
    command(&p, &mut h, "start", None);
    let enroll = calls(&h, "POST /quests/2/enroll");
    assert_eq!(enroll.len(), 1);
    assert_eq!(enroll[0], Some(j(r#"{"location":11,"is_targeted":false}"#)));
    // 404 (Rec's default): the quest is skipped and its card fails.
    let s = st.lock();
    assert!(s.skipped.contains("2"));
    assert_eq!(s.cards["2"].status, Status::Failed);
    assert!(h.logs.iter().any(|l| l.contains("unavailable (404)")));
}

#[test]
fn auto_enroll_off_leaves_the_quest_pending() {
    let (p, st) = plugin();
    let q = quest("3", r#"{"WATCH_VIDEO":{"target":30}}"#, false, "");
    let mut h = Rec::default().with("autoEnroll", Val::Bool(false)).answer(
        "GET",
        "/quests/@me",
        200,
        &quests_body(&[q]),
    );
    command(&p, &mut h, "start", None);
    assert!(calls(&h, "POST").is_empty());
    let s = st.lock();
    assert_eq!(s.cards["3"].status, Status::Pending);
    assert!(s.cards["3"].enroll_required);
    // Nothing queued: 4–6 s pause before the post-cycle wait.
    let (ms, _) = h
        .scheduled
        .iter()
        .find(|(_, t)| t.ends_with("|cyc|post"))
        .unwrap();
    assert!((4000..=6000).contains(ms));
}

#[test]
fn game_quest_sends_presence_and_desktop_heartbeats_then_restores() {
    let (p, st) = plugin();
    let q = quest("4", r#"{"PLAY_ON_DESKTOP":{"target":900}}"#, true, "");
    let mut h = Rec::default()
        .answer("GET", "/quests/@me", 200, &quests_body(&[q]))
        .answer(
            "GET",
            "/applications/public?application_ids=555",
            200,
            r#"[{"name":"Real Game"}]"#,
        )
        .answer(
            "POST",
            "/quests/4/heartbeat",
            200,
            r#"{"progress":{"PLAY_ON_DESKTOP":{"value":120}}}"#,
        );
    // The client's own presence before the run: a custom status and a real game.
    let mut presence = j(r#"{"op":3,"d":{"since":0,"status":"dnd","afk":false,
        "activities":[{"type":4,"name":"Custom Status","state":"hi"},{"type":0,"name":"Real"}]}}"#);
    let v = p.on_event(
        &mut h,
        Event::Send {
            op: 3,
            payload: &mut presence,
        },
    );
    assert_eq!(v, Verdict::Pass);
    command(&p, &mut h, "start", None);
    assert_eq!(
        calls(&h, "GET /applications/public?application_ids=555").len(),
        1
    );
    // applyExclusive + rpc(): only the fake game plays, the custom status stays.
    let (op, d) = h.sent.last().unwrap().clone();
    assert_eq!(op, 3);
    assert_eq!(d.str_at(&["status"]), Some("dnd"));
    let acts = d.get("activities").unwrap().as_list();
    assert_eq!(acts.len(), 2);
    assert_eq!(acts[0].str_at(&["name"]), Some("Custom Status"));
    assert_eq!(acts[1].str_at(&["name"]), Some("Real Game"));
    assert_eq!(acts[1].str_at(&["application_id"]), Some("555"));
    assert_eq!(acts[1].get("type"), Some(&Val::Int(0)));
    assert_eq!(
        acts[1].at(&["timestamps", "start"]),
        Some(&Val::Int(NOW as i64))
    );
    // A presence the client sends meanwhile keeps the fake game.
    let mut again = j(r#"{"op":3,"d":{"status":"idle","activities":[{"type":0,"name":"Real"}]}}"#);
    assert_eq!(
        p.on_event(
            &mut h,
            Event::Send {
                op: 3,
                payload: &mut again
            }
        ),
        Verdict::Changed
    );
    assert_eq!(
        again.at(&["d", "activities"]).unwrap().as_list()[0].str_at(&["name"]),
        Some("Real Game")
    );
    // The desktop heartbeat the client sends for a running game.
    let beats = calls(&h, "POST /quests/4/heartbeat");
    assert_eq!(
        beats[0],
        Some(j(r#"{"application_id":"555","terminal":false}"#))
    );
    assert_eq!(st.lock().cards["4"].cur, 120.0);
    let task = last_tag(&h, "task");
    let ms = h.scheduled.iter().find(|(_, t)| *t == task).unwrap().0;
    assert!((15_000..=20_000).contains(&ms));
    // The client's own heartbeat (QUESTS_SEND_HEARTBEAT_SUCCESS) moves progress too.
    let mut b = j(r#"{"quest_id":"4","progress":{"PLAY_ON_DESKTOP":{"value":900}}}"#);
    p.on_event(
        &mut h,
        Event::RestResponse {
            method: "POST",
            route: "/quests/4/heartbeat",
            status: 200,
            body: &mut b,
        },
    );
    assert_eq!(st.lock().cards["4"].cur, 900.0);
    // Next wake: target reached, the closing terminal beat, then completion and restore().
    fire(&p, &mut h, &task);
    fire_last(&p, &mut h, "traffic");
    let beats = calls(&h, "POST /quests/4/heartbeat");
    assert_eq!(
        beats.last().unwrap(),
        &Some(j(r#"{"application_id":"555","terminal":true}"#))
    );
    assert_eq!(st.lock().cards["4"].status, Status::Completed);
    let (op, d) = h.sent.last().unwrap().clone();
    assert_eq!(op, 3);
    assert_eq!(
        d.str_at(&["status"]),
        Some("idle"),
        "the latest real presence comes back"
    );
    assert!(st.lock().fake_games.is_empty());
}

#[test]
fn a_refused_desktop_heartbeat_falls_back_to_soft_heartbeats_and_hide_activity_hides() {
    let (p, _st) = plugin();
    let q = quest("14", r#"{"PLAY_ON_DESKTOP_V2":{"target":900}}"#, true, "");
    let mut h = Rec::default().with("hideActivity", Val::Bool(true)).answer(
        "GET",
        "/quests/@me",
        200,
        &quests_body(&[q]),
    );
    command(&p, &mut h, "start", None);
    assert!(h.sent.is_empty(), "hideActivity: no fake presence");
    fire_last(&p, &mut h, "task");
    fire_last(&p, &mut h, "traffic");
    let beats = calls(&h, "POST /quests/14/heartbeat");
    assert_eq!(
        beats[0],
        Some(j(r#"{"application_id":"555","terminal":false}"#))
    );
    assert_eq!(
        beats[1],
        Some(j(r#"{"stream_key":"call:14:1","terminal":false}"#))
    );
    assert!(
        h.logs
            .iter()
            .any(|l| l.contains("soft heartbeats from now on"))
    );
}

#[test]
fn stream_quest_uses_your_voice_channel_stream_key() {
    let (p, _st) = plugin();
    let q = quest("15", r#"{"STREAM_ON_DESKTOP":{"target":900}}"#, true, "");
    let mut h = Rec::default().answer("GET", "/quests/@me", 200, &quests_body(&[q]));
    h.self_id = Some(77);
    let mut join =
        j(r#"{"op":4,"d":{"guild_id":"9","channel_id":"8","self_mute":false,"self_deaf":false}}"#);
    p.on_event(
        &mut h,
        Event::Send {
            op: 4,
            payload: &mut join,
        },
    );
    command(&p, &mut h, "start", None);
    let beats = calls(&h, "POST /quests/15/heartbeat");
    assert_eq!(
        beats[0],
        Some(j(r#"{"stream_key":"guild:9:8:77","terminal":false}"#))
    );
    // A DM call: `call:{channel}:{you}`; leaving voice falls back to the script's soft keys.
    let mut call = j(
        r#"{"op":0,"t":"VOICE_STATE_UPDATE","d":{"user_id":"77","guild_id":null,"channel_id":"5"}}"#,
    );
    p.on_event(
        &mut h,
        Event::Dispatch {
            name: "VOICE_STATE_UPDATE",
            payload: &mut call,
            extra: &mut Vec::new(),
        },
    );
    assert_eq!(_st.lock().stream_key(&h).as_deref(), Some("call:5:77"));
    let mut leave = j(r#"{"op":4,"d":{"guild_id":null,"channel_id":null}}"#);
    p.on_event(
        &mut h,
        Event::Send {
            op: 4,
            payload: &mut leave,
        },
    );
    assert_eq!(_st.lock().stream_key(&h), None);
    // Someone else's voice state changes nothing.
    let mut other = j(r#"{"op":0,"t":"VOICE_STATE_UPDATE","d":{"user_id":"1","channel_id":"5"}}"#);
    p.on_event(
        &mut h,
        Event::Dispatch {
            name: "VOICE_STATE_UPDATE",
            payload: &mut other,
            extra: &mut Vec::new(),
        },
    );
    assert_eq!(_st.lock().stream_key(&h), None);
}

#[test]
fn soft_heartbeat_rotates_the_key_on_404() {
    let (p, _st) = plugin();
    let q = quest("5", r#"{"STREAM_ON_DESKTOP":{"target":900}}"#, true, "");
    let mut h = Rec::default().answer("GET", "/quests/@me", 200, &quests_body(&[q]));
    command(&p, &mut h, "start", None);
    // 404 on the first key: the next beat uses `game:<app id>`.
    fire_last(&p, &mut h, "task");
    fire_last(&p, &mut h, "traffic");
    let beats = calls(&h, "POST /quests/5/heartbeat");
    assert_eq!(beats.len(), 2);
    assert_eq!(
        beats[1],
        Some(j(r#"{"stream_key":"game:555","terminal":false}"#))
    );
}

#[test]
fn activity_quest_heartbeats_on_the_latest_dm() {
    let (p, st) = plugin();
    let mut ready = j(r#"{"op":0,"t":"READY","d":{"private_channels":[
        {"id":"10","type":1,"last_message_id":"100"},{"id":"11","type":1,"last_message_id":"900"}],
        "guilds":[]}}"#);
    let mut h = Rec::default().with("autoStart", Val::Bool(false));
    p.on_event(
        &mut h,
        Event::Dispatch {
            name: "READY",
            payload: &mut ready,
            extra: &mut Vec::new(),
        },
    );
    let q = quest("6", r#"{"PLAY_ACTIVITY":{"target":30}}"#, true, "");
    set_answer(&mut h, "GET", "/quests/@me", &quests_body(&[q]));
    set_answer(&mut h, "POST", "/quests/6/heartbeat", "{}");
    command(&p, &mut h, "start", None);
    let beats = calls(&h, "POST /quests/6/heartbeat");
    let key = beats[0]
        .as_ref()
        .unwrap()
        .str_at(&["stream_key"])
        .unwrap()
        .to_string();
    assert!(key.starts_with("call:11:"), "{key}");
    let suffix: u64 = key.rsplit(':').next().unwrap().parse().unwrap();
    assert!((1000..=9999).contains(&suffix));
    // No progress in the answer: +20 s per beat.
    assert_eq!(st.lock().cards["6"].cur, 20.0);
    fire_last(&p, &mut h, "task");
    fire_last(&p, &mut h, "traffic");
    fire_last(&p, &mut h, "traffic");
    let beats = calls(&h, "POST /quests/6/heartbeat");
    assert_eq!(
        beats.last().unwrap().as_ref().unwrap().get("terminal"),
        Some(&Val::Bool(true))
    );
    assert_eq!(st.lock().cards["6"].status, Status::Completed);
}

#[test]
fn achievement_with_a_non_numeric_app_fails_cannot_auto_complete() {
    let (p, st) = plugin();
    let q = quest("7", r#"{"ACHIEVEMENT_IN_ACTIVITY":{"target":1}}"#, true, "")
        .replace(r#""id":"555""#, r#""id":"5x5""#);
    let mut h = Rec::default().answer("GET", "/quests/@me", 200, &quests_body(&[q]));
    command(&p, &mut h, "start", None);
    assert!(calls(&h, "POST").is_empty());
    assert!(calls(&h, "GET /oauth2").is_empty());
    let s = st.lock();
    assert_eq!(s.cards["7"].status, Status::Failed);
    assert!(s.skipped.contains("7"));
    assert!(
        h.logs
            .iter()
            .any(|l| l.contains("non-numeric appId \"5x5\""))
    );
    assert!(h.logs.iter().any(|l| l.contains("Cannot auto-complete")));
}

#[test]
fn achievement_bypass_runs_the_discord_says_flow_and_revokes_its_grant() {
    let (p, st) = plugin();
    let q = quest("7", r#"{"ACHIEVEMENT_IN_ACTIVITY":{"target":1}}"#, true, "");
    let authorize = "/oauth2/authorize?response_type=code&client_id=555&scope=identify%20applications.commands%20applications.entitlements";
    let mut h = Rec::default()
        .answer("GET", "/quests/@me", 200, &quests_body(&[q]))
        .answer(
            "GET",
            "/oauth2/tokens",
            200,
            r#"[{"id":"old","application":{"id":"555"}},{"id":"x","application":{"id":"9"}}]"#,
        )
        .answer(
            "GET",
            "/applications/public?application_ids=555",
            200,
            r#"[{"name":"Quest App"}]"#,
        )
        .answer(
            "POST",
            authorize,
            200,
            r#"{"location":"https://555.discordsays.com/cb?code=CODE1&state=s"}"#,
        )
        .answer(
            "POST",
            "/applications/555/proxy-tickets",
            200,
            r#"{"ticket":"t/1+"}"#,
        )
        .answer("DELETE", "/oauth2/tokens/new", 204, "{}")
        .answer_fetch(
            "POST",
            "https://555.discordsays.com/.proxy/acf/authorize",
            200,
            r#"{"token":"TOK"}"#,
        )
        .answer_fetch(
            "POST",
            "https://555.discordsays.com/.proxy/acf/quest/progress",
            200,
            "{}",
        );
    command(&p, &mut h, "start", None);
    // No channel: straight to the bypass, which waits 60 s for consent.
    assert_eq!(calls(&h, "GET /oauth2/tokens").len(), 1);
    assert!(
        h.notes.iter().any(|(_, b)| b.contains("Quest App (555)")),
        "{:?}",
        h.notes
    );
    let wait = last_tag(&h, "task");
    assert_eq!(
        h.scheduled.iter().find(|(_, t)| *t == wait).unwrap().0,
        60_000
    );
    assert!(calls(&h, "POST").is_empty());
    // The grant list now also has the run's grant.
    set_answer(
        &mut h,
        "GET",
        "/oauth2/tokens",
        r#"[{"id":"old","application":{"id":"555"}},{"id":"new","application":{"id":"555"}}]"#,
    );
    assert_eq!(
        command(&p, &mut h, "consent", Some("1")),
        h.t("quest_master-consent-other", &[("id", "555")])
    );
    command(&p, &mut h, "consent", Some("555"));
    let auth = calls(&h, &format!("POST {authorize}"));
    assert_eq!(
        auth[0],
        Some(j(
            r#"{"permissions":"0","authorize":true,"integration_type":1,
            "location_context":{"guild_id":"10000","channel_id":"10000","channel_type":10000}}"#
        ))
    );
    assert_eq!(
        calls(&h, "POST /applications/555/proxy-tickets")[0],
        Some(j("{}"))
    );
    let referrer = "https://555.discordsays.com/?instance_id=example-cl-instance&platform=desktop&discord_proxy_ticket=t%2F1%2B";
    let (url, headers, body) = &h.fetch_calls[0];
    assert_eq!(url, "POST https://555.discordsays.com/.proxy/acf/authorize");
    let hv = |hs: &Vec<(String, String)>, n: &str| {
        hs.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone())
    };
    assert_eq!(
        hv(headers, "Content-Type").as_deref(),
        Some("application/json")
    );
    assert_eq!(hv(headers, "X-Auth-Token").as_deref(), Some(""));
    assert_eq!(hv(headers, "X-Discord-Quest-ID").as_deref(), Some("7"));
    assert_eq!(hv(headers, "Referer").as_deref(), Some(referrer));
    assert_eq!(body, &Some(j(r#"{"code":"CODE1"}"#)));
    let (url, headers, body) = &h.fetch_calls[1];
    assert_eq!(
        url,
        "POST https://555.discordsays.com/.proxy/acf/quest/progress"
    );
    assert_eq!(hv(headers, "X-Auth-Token").as_deref(), Some("TOK"));
    assert_eq!(body, &Some(j(r#"{"progress":1}"#)));
    // finally: only the grant the run created is revoked.
    assert_eq!(calls(&h, "DELETE /oauth2/tokens/").len(), 1);
    assert_eq!(calls(&h, "DELETE /oauth2/tokens/new").len(), 1);
    assert!(
        h.logs
            .iter()
            .any(|l| l.contains("completed via Discord Says"))
    );
    assert_eq!(st.lock().cards["7"].status, Status::Completed);
}

#[test]
fn achievement_consent_timeout_or_deny_still_cleans_up_and_fails() {
    let (p, st) = plugin();
    let qs = [
        quest("7", r#"{"ACHIEVEMENT_IN_ACTIVITY":{"target":1}}"#, true, ""),
        quest("8", r#"{"ACHIEVEMENT_IN_ACTIVITY":{"target":1}}"#, true, ""),
    ];
    let mut h = Rec::default()
        .with("concurrentGames", Val::Int(1))
        .answer("GET", "/quests/@me", 200, &quests_body(&qs))
        .answer("GET", "/oauth2/tokens", 200, "[]");
    command(&p, &mut h, "start", Some("7"));
    // No answer in 60 s: declined, the grants are checked again, the quest fails.
    fire_last(&p, &mut h, "task");
    assert_eq!(calls(&h, "GET /oauth2/tokens").len(), 2);
    assert!(h.logs.iter().any(|l| l.contains("Consent declined")));
    assert!(calls(&h, "POST").is_empty());
    assert_eq!(st.lock().cards["7"].status, Status::Failed);
    assert_eq!(
        command(&p, &mut h, "deny", None),
        h.t("quest_master-consent-none", &[])
    );
    // `deny` answers a waiting prompt the same way.
    command(&p, &mut h, "retry", None);
    fire_last(&p, &mut h, "open");
    command(&p, &mut h, "start", Some("8"));
    assert!(st.lock().consent_wait.is_some());
    command(&p, &mut h, "deny", None);
    assert!(st.lock().consent_wait.is_none());
    assert_eq!(st.lock().cards["8"].status, Status::Failed);
    assert!(h.fetch_calls.is_empty());
}

#[test]
fn a_429_is_retried_after_retry_after_and_the_local_limit_too() {
    let (p, st) = plugin();
    let q = quest("8", r#"{"WATCH_VIDEO":{"target":30}}"#, true, "");
    let mut h = Rec::default()
        .answer("GET", "/quests/@me", 200, &quests_body(&[q]))
        .answer(
            "POST",
            "/quests/8/video-progress",
            429,
            r#"{"retry_after":2.5,"global":false}"#,
        );
    command(&p, &mut h, "start", None);
    assert!(
        h.logs
            .iter()
            .any(|l| l.contains("Retry 1/3 in 2.5s (HTTP 429)")),
        "{:?}",
        h.logs
    );
    let rt = last_tag(&h, "rt");
    let ms = h.scheduled.iter().find(|(_, t)| *t == rt).unwrap().0;
    assert!((2700..=3300).contains(&ms), "{ms}");
    assert_eq!(st.lock().traffic.parked.len(), 1);
    // Due: back in the queue; the queue is still in its pause, so it waits its turn.
    fire(&p, &mut h, &rt);
    assert_eq!(st.lock().traffic.queue.len(), 1);
    // The local limiter's refusal is a 429 with `retry_after` X.
    assert_eq!(
        retry_in_ms("rate limited: at most 5 Discord API calls per 5 s per plugin, retry in 1.5s"),
        Some(1500)
    );
    assert_eq!(retry_in_ms("nope"), None);
}

#[test]
fn three_retries_then_the_task_counts_a_failure() {
    let (p, st) = plugin();
    let q = quest("9", r#"{"WATCH_VIDEO":{"target":30}}"#, true, "");
    let mut h = Rec::default()
        .answer("GET", "/quests/@me", 200, &quests_body(&[q]))
        .answer(
            "POST",
            "/quests/9/video-progress",
            503,
            r#"{"message":"down"}"#,
        );
    command(&p, &mut h, "start", None);
    for _ in 0..3 {
        fire_last(&p, &mut h, "rt");
        fire_last(&p, &mut h, "traffic");
    }
    assert_eq!(calls(&h, "POST /quests/9/video-progress").len(), 4);
    assert!(
        h.logs
            .iter()
            .any(|l| l.contains("Request to /quests/9/video-progress failed: down"))
    );
    assert_eq!(st.lock().runs["9"].fails, 1);
}

#[test]
fn auto_claim_claims_after_completion() {
    let (p, st) = plugin();
    let q = quest("12", r#"{"WATCH_VIDEO":{"target":5}}"#, true, "");
    let mut h = Rec::default()
        .with("autoClaim", Val::Bool(true))
        .answer("GET", "/quests/@me", 200, &quests_body(&[q]))
        .answer(
            "POST",
            "/quests/12/video-progress",
            200,
            r#"{"completed_at":"x"}"#,
        )
        .answer(
            "POST",
            "/quests/12/claim-reward",
            200,
            r#"{"claimed_at":"y"}"#,
        );
    command(&p, &mut h, "start", None);
    let claim = last_tag(&h, "claim");
    let ms = h.scheduled.iter().find(|(_, t)| *t == claim).unwrap().0;
    assert!((2500..=6000).contains(&ms));
    fire(&p, &mut h, &claim);
    let body = calls(&h, "POST /quests/12/claim-reward");
    assert_eq!(
        body[0].as_ref().unwrap().get("location"),
        Some(&Val::Int(11))
    );
    assert_eq!(st.lock().cards["12"].status, Status::Claimed);
}

#[test]
fn stop_cancels_timers_and_the_off_path_does_nothing() {
    let (p, st) = plugin();
    let q = quest("13", r#"{"WATCH_VIDEO":{"target":30}}"#, true, "");
    let mut h = Rec::default().answer("GET", "/quests/@me", 200, &quests_body(&[q]));
    command(&p, &mut h, "start", None);
    assert_eq!(
        command(&p, &mut h, "stop", None),
        h.t("quest_master-stopping", &[])
    );
    assert!(!h.cancelled.is_empty());
    assert_eq!(st.lock().phase, Phase::Idle);
    // A stale timer from the old session is ignored.
    let before = h.rest_calls.len();
    fire_last(&p, &mut h, "task");
    assert_eq!(h.rest_calls.len(), before);
    assert_eq!(
        command(&p, &mut h, "stop", None),
        h.t("quest_master-not-running", &[])
    );

    // Plugin off: the chain never delivers its timers, the command answers by itself.
    let (p2, st2) = plugin();
    let chain = Chain::new(vec![Box::new(p2)]);
    chain.set_enabled(Vec::<String>::new());
    let mut h2 = Rec::default();
    h2.plugins.insert(PLUGIN_ID.into(), false);
    let mut f = Rec::timer_frame("1|cyc|next");
    chain.dispatch(&mut h2, TIMER_EVENT, &mut f, &mut Vec::new());
    assert!(h2.rest_calls.is_empty());
    let cmd = QuestMaster::new().command();
    let text = cmd.run(
        &mut h2,
        &Args {
            options: vec![("action".into(), "start".into())],
        },
    );
    assert_eq!(text, h2.t("plugin-off", &[("id", PLUGIN_ID)]));
    assert!(st2.lock().pending.is_empty());
}

#[test]
fn list_status_and_claim_commands_answer_from_the_dashboard_state() {
    let (p, st) = plugin();
    let mut h = Rec::default();
    assert_eq!(
        command(&p, &mut h, "list", None),
        h.t("quest_master-list-empty", &[])
    );
    let qs = [
        quest("20", r#"{"WATCH_VIDEO":{"target":30}}"#, true, ""),
        quest(
            BLACKLISTED_QUEST_ID,
            r#"{"WATCH_VIDEO":{"target":30}}"#,
            true,
            "",
        ),
    ];
    set_answer(&mut h, "GET", "/quests/@me", &quests_body(&qs));
    command(&p, &mut h, "list", None);
    let list = command(&p, &mut h, "list", None);
    assert!(
        list.contains("`20`") && list.contains("700 Orbs") && list.contains("ORBS"),
        "{list}"
    );
    assert!(!list.contains(BLACKLISTED_QUEST_ID));
    st.lock().cards.insert(
        "20".into(),
        Card {
            name: "Quest 20".into(),
            kind: Kind::Video,
            cur: 30.0,
            max: 30.0,
            status: Status::Completed,
            claimable: true,
            claim: ClaimState::None,
            enroll_required: false,
            order: 0,
        },
    );
    assert!(command(&p, &mut h, "status", None).contains("Quest 20"));
    assert_eq!(
        command(&p, &mut h, "claim", None),
        h.t("quest_master-claim-missing", &[])
    );
    command(&p, &mut h, "claim", Some("20"));
    // Rec answers 404: the claim needs action (captcha) in Discord.
    assert_eq!(st.lock().cards["20"].claim, ClaimState::Failed);
    assert_eq!(calls(&h, "POST /quests/20/claim-reward").len(), 1);
    assert!(command(&p, &mut h, "bogus", None).contains("bogus"));
}

#[test]
fn autostart_opens_the_picker_and_logs_to_the_file() {
    let (p, st) = plugin();
    let q = quest("30", r#"{"WATCH_VIDEO":{"target":30}}"#, true, "");
    let mut h = Rec::default().answer("GET", "/quests/@me", 200, &quests_body(&[q]));
    let mut ready = j(
        r#"{"op":0,"t":"READY","d":{"guilds":[{"id":"1","channels":[{"id":"50","type":2,"position":3}]}]}}"#,
    );
    p.on_event(
        &mut h,
        Event::Dispatch {
            name: "READY",
            payload: &mut ready,
            extra: &mut Vec::new(),
        },
    );
    let s = st.lock();
    assert_eq!(s.phase, Phase::Picker);
    assert_eq!(s.heartbeat_channel(), Some(50));
    assert!(calls(&h, "POST").is_empty());
    assert!(h.logs.iter().any(|l| l.contains("questmaster.log")));
    drop(s);
    // `start <id>` picks only that quest.
    command(&p, &mut h, "start", Some("30"));
    assert_eq!(st.lock().phase, Phase::Running);
}

#[test]
fn nothing_to_do_and_malformed_payloads_do_not_panic() {
    let (p, st) = plugin();
    let mut h = Rec::default().answer("GET", "/quests/@me", 200, r#"{"quests":"nope"}"#);
    command(&p, &mut h, "start", None);
    assert_eq!(st.lock().phase, Phase::Finished);
    assert_eq!(h.notes.len(), 1);
    // Expired, completed and config-less quests never run.
    let mut done = j(&quest("40", r#"{"WATCH_VIDEO":{"target":30}}"#, true, ""));
    done.get_mut("user_status")
        .unwrap()
        .set("completed_at", Val::Str("2027-01-01T00:00:00Z".into()));
    let no_expiry =
        r#"{"id":"41","config":{"task_config":{"tasks":{"WATCH_VIDEO":{"target":30}}}}}"#;
    let body = format!(
        r#"{{"quests":[{},{no_expiry},{{"id":"42"}},5,null]}}"#,
        done.to_json()
    );
    set_answer(&mut h, "GET", "/quests/@me", &body);
    command(&p, &mut h, "retry", None);
    fire_last(&p, &mut h, "open");
    assert_eq!(st.lock().phase, Phase::Finished);
    // Odd frames and answers.
    for mut f in [
        j(r#"{"op":0,"t":"PHOECORD_TIMER","d":{}}"#),
        j("null"),
        Rec::timer_frame("x|y"),
    ] {
        p.on_event(
            &mut h,
            Event::Dispatch {
                name: TIMER_EVENT,
                payload: &mut f,
                extra: &mut Vec::new(),
            },
        );
    }
    for mut b in [j("[]"), j("7"), j(r#"{"progress":"x"}"#)] {
        p.on_event(
            &mut h,
            Event::RestResponse {
                method: "POST",
                route: "/quests/1/heartbeat",
                status: 200,
                body: &mut b,
            },
        );
        p.on_event(
            &mut h,
            Event::RestResponse {
                method: "GET",
                route: "/quests/@me",
                status: 200,
                body: &mut b,
            },
        );
    }
    let mut ready = j(
        r#"{"op":0,"t":"READY","d":{"private_channels":5,"guilds":[{"channels":"x"},{"id":"2"}]}}"#,
    );
    p.on_event(
        &mut h,
        Event::Dispatch {
            name: "READY",
            payload: &mut ready,
            extra: &mut Vec::new(),
        },
    );
    let mut weird = j(r#"{"data":7}"#);
    p.on_event(
        &mut h,
        Event::RestRequest {
            method: "POST",
            route: "/interactions",
            body: Some(&mut weird),
        },
    );
}

#[test]
fn helpers_match_the_script() {
    assert_eq!(parse_iso("1970-01-01T00:00:01.5+00:00"), Some(1500));
    assert_eq!(parse_iso("2024-02-29T12:00:00Z"), Some(1_709_208_000_000));
    assert_eq!(
        parse_iso("2024-02-29T14:00:00+02:00"),
        Some(1_709_208_000_000)
    );
    assert_eq!(parse_iso("garbage"), None);
    assert_eq!(fmt_iso(1_709_208_000_123), "2024-02-29T12:00:00.123Z");
    assert_eq!(sanitize("<!DOCTYPE html><p>x", 10), "[HTML error page]");
    assert_eq!(sanitize("a  b\n c", 180), "a b c");
    assert_eq!(sanitize("abcdef", 3), "abc…");
    let q = j(r#"{"config":{}}"#);
    assert!(!not_expired(&q, NOW), "no expiresAt is time 0");
    let q = j(r#"{"config":{"expiresAt":"soon"}}"#);
    assert!(not_expired(&q, NOW), "an unreadable date is NaN");
    let mut r = Rng::seeded();
    for _ in 0..1000 {
        assert!((19_000..=22_000).contains(&r.rnd(19_000, 22_000)));
    }
    assert_eq!(n(900.0), "900");
    assert_eq!(n(1.5), "1.5");
}
