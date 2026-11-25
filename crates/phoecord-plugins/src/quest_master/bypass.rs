//! The ACHIEVEMENT bypass (`Tasks.bypassAchievement` and native.ts), one call per step so the
//! rate limiter can pause it: snapshot the app's OAuth grants, ask for consent, authorize the app,
//! get a proxy ticket, trade the code for a discordsays token, report the target progress, then
//! revoke the grant the run created.

use super::*;

/// Scopes the script authorizes (shown in the consent prompt too).
const SCOPES: [&str; 3] = [
    "identify",
    "applications.commands",
    "applications.entitlements",
];
/// `Consent.TIMEOUT`.
const CONSENT_TIMEOUT_MS: u64 = 60_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Step {
    Snapshot,
    AppName,
    Consent,
    Authorize,
    Ticket,
    DsAuthorize,
    DsProgress,
    Cleanup,
    Revoke,
    Done,
}

/// One bypass in progress.
#[derive(Clone, Debug)]
pub(super) struct Bypass {
    pub(super) app: String,
    pub(super) step: Step,
    /// Grants of this app before the run (`preGrantIds`); `None` until the snapshot worked.
    pub(super) pre: Option<Vec<String>>,
    app_name: Option<String>,
    code: String,
    referrer: String,
    token: String,
    ok: bool,
    revoke: Vec<String>,
}

/// JS `encodeURIComponent`.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `new URL(location).searchParams.get("code")`.
fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1.split('#').next()?;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == name).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16)
                {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Why a step failed, the way the script's catch block words it (`parts.join(" — ")`).
struct Failure {
    status: Option<u16>,
    code: Option<i64>,
    message: String,
}

impl Failure {
    fn plain(message: impl Into<String>) -> Failure {
        Failure {
            status: None,
            code: None,
            message: message.into(),
        }
    }
    fn api(e: &ApiErr) -> Failure {
        let message = e
            .body_field("message")
            .and_then(text)
            .unwrap_or_else(|| e.message.clone());
        Failure {
            status: e.status,
            code: e.body_field("code").and_then(Val::as_i64),
            message,
        }
    }
    fn text(&self) -> String {
        let mut parts = Vec::new();
        if let Some(s) = self.status {
            parts.push(format!("HTTP {s}"));
        }
        if let Some(c) = self.code {
            parts.push(format!("code {c}"));
        }
        if !self.message.is_empty() {
            parts.push(self.message.clone());
        }
        if parts.is_empty() {
            "unknown".into()
        } else {
            parts.join(" — ")
        }
    }
}

/// What one step did.
enum Next {
    /// Go on with the next step now.
    Go,
    /// Wait (rate limit or consent); the task is parked.
    Wait,
}

impl Engine {
    /// Start `bypassAchievement` for a task (the heartbeat path did not finish it).
    pub(super) fn ach_bypass(&mut self, host: &mut dyn Host, mut r: Run) {
        let (id, lane) = (r.info.id.clone(), r.lane);
        if !self.running {
            return self.task_end(host, &id, lane);
        }
        let app = q_app_id(&r.quest).unwrap_or_default();
        if app.is_empty() {
            return self.bypass_failed(host, r);
        }
        if !app.bytes().all(|b| b.is_ascii_digit()) {
            self.say(host, Lvl::Warn, "qm-bypass-non-numeric", &[("id", &app)]);
            return self.bypass_failed(host, r);
        }
        r.bypass = Some(Bypass {
            app,
            step: Step::Snapshot,
            pre: None,
            app_name: None,
            code: String::new(),
            referrer: String::new(),
            token: String::new(),
            ok: false,
            revoke: Vec::new(),
        });
        self.bypass_run(host, r);
    }

    /// Run steps until one has to wait or the bypass is done.
    pub(super) fn bypass_run(&mut self, host: &mut dyn Host, mut r: Run) {
        loop {
            let Some(step) = r.bypass.as_ref().map(|b| b.step) else {
                return self.bypass_failed(host, r);
            };
            if step == Step::Done {
                let ok = r.bypass.take().is_some_and(|b| b.ok);
                if ok {
                    return self.finish(host, r);
                }
                return self.bypass_failed(host, r);
            }
            let (next, back) = self.bypass_step(host, &mut r, step);
            match next {
                Next::Go => {}
                Next::Wait => {
                    if let Some(ms) = back {
                        self.sleep(host, r, ms, Wake::Bypass);
                    }
                    return;
                }
            }
        }
    }

    /// The script's skip after a failed bypass.
    fn bypass_failed(&mut self, host: &mut dyn Host, r: Run) {
        let (id, lane) = (r.info.id.clone(), r.lane);
        if !self.running {
            return self.task_end(host, &id, lane);
        }
        let name = r.info.name.clone();
        self.say(host, Lvl::Warn, "qm-skip-no-path", &[("name", &name)]);
        self.fail(host, r, "qm-reason-cannot", &[]);
    }

    /// One step. `(Wait, Some(ms))` = retry this step after a rate limit; `(Wait, None)` = the
    /// task is parked elsewhere (consent).
    fn bypass_step(&mut self, host: &mut dyn Host, r: &mut Run, step: Step) -> (Next, Option<u64>) {
        let Some(b) = r.bypass.as_mut() else {
            return (Next::Go, None);
        };
        let app = b.app.clone();
        macro_rules! limited {
            ($res:expr) => {
                match $res {
                    Err(e) if e.rate_limited().is_some() => {
                        return (Next::Wait, e.rate_limited());
                    }
                    other => other,
                }
            };
        }
        match step {
            Step::Snapshot => {
                match limited!(call(host, "GET", "/oauth2/tokens", None)) {
                    Ok(list) => {
                        b.pre = Some(grants_of(&list, &app));
                        b.step = Step::AppName;
                    }
                    Err(e) => {
                        let m = e.msg();
                        b.step = Step::Done;
                        self.say(
                            host,
                            Lvl::Warn,
                            "qm-bypass-snapshot-failed",
                            &[("error", &m)],
                        );
                    }
                }
                (Next::Go, None)
            }
            Step::AppName => {
                let route = format!("/applications/public?application_ids={app}");
                if let Ok(body) = limited!(call(host, "GET", &route, None)) {
                    b.app_name = body
                        .as_list()
                        .first()
                        .and_then(|a| get(a, "name"))
                        .and_then(text);
                }
                b.step = Step::Consent;
                (Next::Go, None)
            }
            Step::Consent => {
                if self.granted.contains(&app) {
                    b.step = Step::Authorize;
                    let name = r.info.name.clone();
                    self.say(host, Lvl::Info, "qm-bypass-trying", &[("name", &name)]);
                    return (Next::Go, None);
                }
                // Consent.ask: a newer prompt replaces an older one, which counts as "no".
                if let Some((old, _)) = self.consent_wait.take()
                    && let Some(mut o) = self.runs.remove(&old)
                {
                    self.consent_answer(host, &mut o, false);
                    self.bypass_run(host, o);
                }
                let label = match &b.app_name {
                    Some(n) => format!("{n} ({app})"),
                    None => format!("App {app}"),
                };
                let scopes = SCOPES.join(", ");
                self.say(
                    host,
                    Lvl::Warn,
                    "qm-consent-ask",
                    &[("app", &label), ("id", &app), ("scopes", &scopes)],
                );
                let (title, body) = (
                    host.t("qm-consent-title", &[]),
                    host.t("qm-consent-notify", &[("app", &label), ("id", &app)]),
                );
                host.notify(&title, &body);
                self.consent_wait = Some((r.info.id.clone(), app));
                (Next::Wait, Some(CONSENT_TIMEOUT_MS))
            }
            Step::Authorize => {
                let route = format!(
                    "/oauth2/authorize?response_type=code&client_id={app}&scope={}",
                    SCOPES.join("%20")
                );
                let body = obj(&[
                    ("permissions", Val::Str("0".into())),
                    ("authorize", Val::Bool(true)),
                    ("integration_type", Val::Int(1)),
                    (
                        "location_context",
                        obj(&[
                            ("guild_id", Val::Str("10000".into())),
                            ("channel_id", Val::Str("10000".into())),
                            ("channel_type", Val::Int(10000)),
                        ]),
                    ),
                ]);
                let res = limited!(call(host, "POST", &route, Some(&body)));
                let code = match res {
                    Ok(v) => match get(&v, "location").and_then(text) {
                        None => Err(Failure::plain("no location in /oauth2/authorize response")),
                        Some(loc) => query_param(&loc, "code")
                            .filter(|c| !c.is_empty())
                            .ok_or_else(|| Failure::plain("no code in authorize location")),
                    },
                    Err(e) => Err(Failure::api(&e)),
                };
                if let Ok(c) = &code {
                    let c = c.clone();
                    if let Some(b) = r.bypass.as_mut() {
                        b.code = c;
                        b.step = Step::Ticket;
                    }
                }
                self.bypass_outcome(host, r, code.map(|_| ()))
            }
            Step::Ticket => {
                let route = format!("/applications/{app}/proxy-tickets");
                let res = limited!(call(host, "POST", &route, Some(&Val::map())));
                let ticket = match res {
                    Ok(v) => get(&v, "ticket")
                        .and_then(text)
                        .ok_or_else(|| Failure::plain("no proxy ticket")),
                    Err(e) => Err(Failure::api(&e)),
                };
                if let Ok(t) = &ticket {
                    let t = t.clone();
                    if let Some(b) = r.bypass.as_mut() {
                        b.referrer = format!(
                            "https://{app}.discordsays.com/?instance_id=example-cl-instance&platform=desktop&discord_proxy_ticket={}",
                            encode_uri_component(&t)
                        );
                        b.step = Step::DsAuthorize;
                    }
                }
                self.bypass_outcome(host, r, ticket.map(|_| ()))
            }
            Step::DsAuthorize => {
                let url = format!("https://{app}.discordsays.com/.proxy/acf/authorize");
                let body = obj(&[("code", Val::Str(b.code.clone()))]);
                let headers = [
                    ("Content-Type", "application/json"),
                    ("X-Auth-Token", ""),
                    ("X-Discord-Quest-ID", r.info.id.as_str()),
                    ("Referer", b.referrer.as_str()),
                ];
                let res = discordsays(host, &url, &headers, &body).and_then(|resp| {
                    // The host parses JSON answers; text means `JSON.parse` would throw.
                    match resp.body.clone() {
                        None => Err(Failure::plain(format!(
                            "discordsays returned non-JSON: {}",
                            resp.text
                                .unwrap_or_default()
                                .chars()
                                .take(120)
                                .collect::<String>()
                        ))),
                        Some(v) => get(&v, "token")
                            .and_then(text)
                            .filter(|t| !t.is_empty())
                            .ok_or_else(|| Failure::plain("no discordsays token")),
                    }
                });
                if let Ok(t) = &res {
                    let t = t.clone();
                    if let Some(b) = r.bypass.as_mut() {
                        b.token = t;
                        b.step = Step::DsProgress;
                    }
                }
                self.bypass_outcome(host, r, res.map(|_| ()))
            }
            Step::DsProgress => {
                let url = format!("https://{app}.discordsays.com/.proxy/acf/quest/progress");
                let body = obj(&[("progress", ts_val(r.info.target))]);
                let headers = [
                    ("Content-Type", "application/json"),
                    ("X-Auth-Token", b.token.as_str()),
                    ("X-Discord-Quest-ID", r.info.id.as_str()),
                    ("Referer", b.referrer.as_str()),
                ];
                let res = discordsays(host, &url, &headers, &body).map(|_| ());
                let ok = res.is_ok();
                let out = self.bypass_outcome(host, r, res.map(|()| {}));
                if ok {
                    if let Some(b) = r.bypass.as_mut() {
                        b.ok = true;
                        b.step = Step::Cleanup;
                    }
                    let name = r.info.name.clone();
                    self.say(host, Lvl::Success, "qm-bypass-success", &[("name", &name)]);
                }
                out
            }
            Step::Cleanup => {
                let Some(pre) = b.pre.clone() else {
                    b.step = Step::Done;
                    return (Next::Go, None);
                };
                match limited!(call(host, "GET", "/oauth2/tokens", None)) {
                    Ok(list) => {
                        b.revoke = grants_of(&list, &app)
                            .into_iter()
                            .filter(|g| !pre.contains(g))
                            .collect();
                        b.step = Step::Revoke;
                    }
                    Err(e) => {
                        b.step = Step::Done;
                        let m = e.msg();
                        self.say(
                            host,
                            Lvl::Debug,
                            "qm-bypass-cleanup-failed",
                            &[("error", &m)],
                        );
                    }
                }
                (Next::Go, None)
            }
            Step::Revoke => {
                let Some(grant) = b.revoke.first().cloned() else {
                    b.step = Step::Done;
                    return (Next::Go, None);
                };
                let route = format!("/oauth2/tokens/{grant}");
                match limited!(call(host, "DELETE", &route, None)) {
                    Ok(_) => {
                        b.revoke.remove(0);
                    }
                    Err(e) => {
                        // One failed revoke ends the cleanup, like the script's try block.
                        b.step = Step::Done;
                        let m = e.msg();
                        self.say(
                            host,
                            Lvl::Debug,
                            "qm-bypass-cleanup-failed",
                            &[("error", &m)],
                        );
                    }
                }
                (Next::Go, None)
            }
            Step::Done => (Next::Go, None),
        }
    }

    /// A step's result: on failure log the script's message and go to the cleanup.
    fn bypass_outcome(
        &mut self,
        host: &mut dyn Host,
        r: &mut Run,
        res: Result<(), Failure>,
    ) -> (Next, Option<u64>) {
        if let Err(f) = res {
            let name = r.info.name.clone();
            if f.code == Some(50165) {
                self.say(host, Lvl::Warn, "qm-bypass-age-gated", &[("name", &name)]);
            } else {
                let t = f.text();
                self.say(host, Lvl::Warn, "qm-bypass-failed", &[("error", &t)]);
            }
            if let Some(b) = r.bypass.as_mut() {
                b.ok = false;
                b.step = Step::Cleanup;
            }
        }
        (Next::Go, None)
    }

    /// The consent prompt was answered (or timed out, or was replaced).
    pub(super) fn consent_answer(&mut self, host: &mut dyn Host, r: &mut Run, yes: bool) {
        let name = r.info.name.clone();
        let Some(b) = r.bypass.as_mut() else {
            return;
        };
        if b.step != Step::Consent {
            return;
        }
        if yes {
            b.step = Step::Authorize;
            self.say(host, Lvl::Info, "qm-bypass-trying", &[("name", &name)]);
        } else {
            // Declined: the script's finally still checks the grants.
            b.step = Step::Cleanup;
            self.say(host, Lvl::Warn, "qm-consent-declined", &[("name", &name)]);
        }
    }

    /// `/phoecord quests consent <app>` / `consent-always <app>` / `deny`.
    pub(super) fn consent_cmd(
        &mut self,
        host: &mut dyn Host,
        app: Option<String>,
        yes: bool,
        remember: bool,
    ) {
        let Some((qid, waiting_app)) = self.consent_wait.clone() else {
            return;
        };
        if app.as_ref().is_some_and(|a| *a != waiting_app) {
            return;
        }
        self.consent_wait = None;
        if yes && remember {
            self.granted.insert(waiting_app);
        }
        let tag = format!("{}|task|{qid}", self.session);
        host.cancel_schedule(&tag);
        self.tags.remove(&tag);
        if let Some(mut r) = self.runs.remove(&qid) {
            self.consent_answer(host, &mut r, yes);
            self.bypass_run(host, r);
        }
    }

    /// `stop` during a bypass: the grant it made is revoked at once (the script's finally).
    pub(super) fn bypass_abort(&mut self, host: &mut dyn Host) {
        let pending: Vec<(String, Vec<String>)> = self
            .runs
            .values()
            .filter_map(|r| r.bypass.as_ref())
            .filter(|b| {
                matches!(
                    b.step,
                    Step::Ticket
                        | Step::DsAuthorize
                        | Step::DsProgress
                        | Step::Cleanup
                        | Step::Revoke
                )
            })
            .filter_map(|b| b.pre.clone().map(|p| (b.app.clone(), p)))
            .collect();
        for (app, pre) in pending {
            if let Ok(list) = call(host, "GET", "/oauth2/tokens", None) {
                for g in grants_of(&list, &app)
                    .into_iter()
                    .filter(|g| !pre.contains(g))
                {
                    let _ = call(host, "DELETE", &format!("/oauth2/tokens/{g}"), None);
                }
            }
        }
        self.consent_wait = None;
    }
}

/// Ids of the OAuth grants of one app in a `GET /oauth2/tokens` answer.
fn grants_of(list: &Val, app: &str) -> Vec<String> {
    list.as_list()
        .iter()
        .filter(|t| {
            t.get("application")
                .and_then(|a| get(a, "id"))
                .and_then(text)
                .as_deref()
                == Some(app)
        })
        .filter_map(|t| get(t, "id").and_then(text))
        .collect()
}

/// A discordsays POST (native.ts `postJson`): a non-2xx answer is a failure with its body.
fn discordsays(
    host: &mut dyn Host,
    url: &str,
    headers: &[(&str, &str)],
    body: &Val,
) -> Result<phoecord_core::plugin::HostResponse, Failure> {
    match host.fetch_with("POST", url, headers, Some(body)) {
        Ok(r) if (200..300).contains(&r.status) => Ok(r),
        Ok(r) => {
            // native.ts hands back the body as text, so the script's catch shows
            // `JSON.stringify({status, body})` cut to 200 characters.
            let raw = match &r.body {
                Some(b) => b.to_json().to_string(),
                None => r.text.clone().unwrap_or_default(),
            };
            let shown = obj(&[
                ("status", Val::Int(i64::from(r.status))),
                ("body", Val::Str(raw)),
            ])
            .to_json()
            .to_string();
            Err(Failure {
                status: Some(r.status),
                code: None,
                message: shown.chars().take(200).collect(),
            })
        }
        Err(e) => Err(Failure::plain(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_helpers_match_js() {
        assert_eq!(encode_uri_component("a b/c+d=é"), "a%20b%2Fc%2Bd%3D%C3%A9");
        assert_eq!(encode_uri_component("A-z_0.!~*'()"), "A-z_0.!~*'()");
        assert_eq!(
            query_param("https://x.example/cb?state=1&code=ab%2Bc#f", "code").as_deref(),
            Some("ab+c")
        );
        assert_eq!(query_param("https://x.example/cb", "code"), None);
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
    }
}
