//! Everything the window does to the machine, run off the UI thread. All of it goes through
//! `phoecord-installer` (the same functions `phoecord setup` / `uninstall` call); this module only
//! adds threads, progress messages and a sandbox for dry runs.

use anyhow::{Context as _, Result, bail};
use phoecord_installer::Channel;
use phoecord_installer::discover::{Client, Source};
use phoecord_installer::install::{Phase, PhaseState};
use phoecord_installer::launch::Occupant;
use phoecord_installer::log::{InstallLog, Step};
use phoecord_installer::system::{Env, Os, RealSystem, System};
use phoecord_installer::uninstall::UninstallReport;
use phoecord_proxy::config::{Config, Paths};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

/// How the window was started.
#[derive(Clone, Debug, Default)]
pub struct Mode {
    /// `--dry-run`: every folder lives under `sandbox`, and the trust store, registry, launchd
    /// and systemd are simulated. Nothing outside `sandbox` changes.
    pub sandbox: Option<PathBuf>,
    /// `--demo`: show two made-up Discord installs (with `--dry-run`, for screenshots).
    pub demo: bool,
    /// `--open <path>`: offer this plugin package for install on start (a file or folder
    /// association can use it too).
    pub open: Option<PathBuf>,
}

impl Mode {
    pub fn from_args(args: &[String]) -> Mode {
        let has = |f: &str| args.iter().any(|a| a == f);
        let sandbox = has("--dry-run").then(|| {
            std::env::var_os("PHOECORD_GUI_SANDBOX")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("phoecord-gui-dry-run"))
        });
        let open = args
            .iter()
            .position(|a| a == "--open")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        Mode {
            demo: has("--demo") && sandbox.is_some(),
            sandbox,
            open,
        }
    }

    pub fn is_dry(&self) -> bool {
        self.sandbox.is_some()
    }

    /// Config and data paths: the real ones, or inside the sandbox.
    pub fn paths(&self) -> Paths {
        match &self.sandbox {
            Some(s) => Paths {
                config: s.join("config/phoecord/config.toml"),
                data: s.join("data/phoecord"),
            },
            None => Paths::resolve(),
        }
    }

    /// Where the installer works. `proxy_exe` is the `phoecord` binary next to this window's
    /// binary (autostart and the shortcut run it), not the window itself.
    pub fn env(&self) -> Result<Env> {
        let mut env = match &self.sandbox {
            Some(s) => Env {
                os: Os::current(),
                data_dir: self.paths().data,
                home: s.join("home"),
                config_home: s.join("config"),
                proxy_exe: PathBuf::new(),
                start_menu: s.join("start-menu"),
                applications_dir: s.join("applications"),
            },
            None => Env::system()?,
        };
        env.proxy_exe = proxy_exe();
        Ok(env)
    }

    pub fn system(&self) -> Box<dyn System> {
        match &self.sandbox {
            Some(s) => Box::new(DrySystem {
                store: s.join("trust-store.txt"),
            }),
            None => Box::new(RealSystem),
        }
    }

    pub fn clients(&self) -> Vec<Client> {
        if self.demo {
            return demo_clients();
        }
        phoecord_installer::discover::discover()
    }
}

/// The `phoecord` binary: `PHOECORD_BIN`, else next to this executable.
pub fn proxy_exe() -> PathBuf {
    if let Some(p) = std::env::var_os("PHOECORD_BIN") {
        return PathBuf::from(p);
    }
    let name = format!("phoecord{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(&name)))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn demo_clients() -> Vec<Client> {
    let c = |channel, version: &str, root: &str| Client {
        channel,
        source: Source::Native,
        root: PathBuf::from(root),
        exe: PathBuf::from(root).join("Discord"),
        version: Some(version.into()),
    };
    vec![
        c(Channel::Stable, "1.0.9172", "/demo/Discord"),
        c(Channel::Canary, "1.0.1355", "/demo/Discord Canary"),
    ]
}

/// Simulated trust store and services for `--dry-run`: the trusted fingerprint lives in a file in
/// the sandbox, everything else succeeds without doing anything.
struct DrySystem {
    store: PathBuf,
}

impl System for DrySystem {
    fn trust_ca(&self, ca_pem: &Path) -> Result<()> {
        std::thread::sleep(Duration::from_millis(700)); // what a system prompt costs, roughly
        let fp = phoecord_proxy::trust::fingerprint(ca_pem)?;
        std::fs::write(&self.store, fp)?;
        Ok(())
    }
    fn untrust_ca(&self) -> Result<()> {
        let _ = std::fs::remove_file(&self.store);
        Ok(())
    }
    fn is_trusted(&self, ca_pem: &Path) -> Result<bool> {
        let fp = phoecord_proxy::trust::fingerprint(ca_pem)?;
        Ok(std::fs::read_to_string(&self.store).is_ok_and(|s| s.trim() == fp))
    }
    fn run_key_set(&self, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
    fn run_key_delete(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn launchctl_load(&self, _: &Path) -> Result<()> {
        Ok(())
    }
    fn launchctl_unload(&self, _: &Path) -> Result<()> {
        Ok(())
    }
    fn systemd_enable(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn systemd_disable(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn stop_proxies(&self, _: &[PathBuf]) -> Result<Vec<u32>> {
        Ok(Vec::new())
    }
    fn shortcut_create(&self, lnk: &Path, _: &Path, _: &str, _: &str) -> Result<()> {
        std::fs::write(lnk, b"dry-run shortcut")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Proxy {
    Running(String),
    Stopped,
    /// Another program holds the port.
    PortTaken,
}

/// One Discord install and what PhoeCord has done for it.
#[derive(Clone, Debug)]
pub struct ClientStatus {
    pub client: Client,
    /// The PhoeCord shortcut for this channel exists (always true on macOS, which has none).
    pub shortcut: bool,
}

/// What the window shows; refreshed every few seconds and after every job.
#[derive(Clone, Debug)]
pub struct Status {
    pub clients: Vec<ClientStatus>,
    /// The CA was trusted by an install (the log says so).
    pub installed: bool,
    /// `Some(false)`: installed, but the CA in the data folder is not trusted now (Repair).
    pub trusted: Option<bool>,
    pub proxy: Proxy,
    pub listen: String,
    pub proxy_exe: PathBuf,
    pub proxy_exe_found: bool,
}

impl Status {
    pub fn client(&self, ch: Channel) -> Option<&ClientStatus> {
        self.clients.iter().find(|c| c.client.channel == ch)
    }

    pub fn needs_repair(&self) -> bool {
        self.installed && self.trusted == Some(false)
    }

    /// PhoeCord is set up for this channel.
    pub fn is_on(&self, ch: Channel) -> bool {
        self.installed && self.client(ch).is_some_and(|c| c.shortcut)
    }
}

pub fn read_status(mode: &Mode) -> Status {
    let paths = mode.paths();
    let listen = Config::load(&paths.config)
        .map(|c| c.listen)
        .unwrap_or_else(|_| Config::default().listen);
    let proxy = match phoecord_installer::launch::probe_proxy(&listen) {
        Occupant::PhoeCord(v) => Proxy::Running(v),
        Occupant::Nobody => Proxy::Stopped,
        Occupant::Other => Proxy::PortTaken,
    };
    let env = mode.env().ok();
    let log = InstallLog::in_dir(&paths.data).read().unwrap_or_default();
    let installed = log.iter().any(|e| matches!(e.step, Step::CaTrusted { .. }));
    let trusted = match (&env, installed) {
        (Some(env), true) => {
            let pem = env.data_dir.join("ca.pem");
            let sys = mode.system();
            Some(!phoecord_installer::ca::is_untrusted(sys.as_ref(), &pem))
        }
        _ => None,
    };
    let mut clients: Vec<ClientStatus> = mode
        .clients()
        .into_iter()
        .map(|client| {
            let shortcut = match &env {
                Some(env) => match phoecord_installer::shortcut::path(env, client.channel) {
                    Some(p) => p.exists(),
                    None => installed,
                },
                None => false,
            };
            ClientStatus { client, shortcut }
        })
        .collect();
    clients.sort_by_key(|c| channel_rank(c.client.channel));
    let proxy_exe = proxy_exe();
    Status {
        clients,
        installed,
        trusted,
        proxy,
        listen,
        proxy_exe_found: mode.is_dry() || proxy_exe.is_file(),
        proxy_exe,
    }
}

pub fn channel_rank(c: Channel) -> u8 {
    match c {
        Channel::Stable => 0,
        Channel::Ptb => 1,
        Channel::Canary => 2,
        Channel::Phoenix => 3,
    }
}

// ---------------------------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------------------------

/// What a job reports back to the window.
#[derive(Debug)]
pub enum Msg {
    Status(Box<Status>),
    /// `None` phase: preparing the config (before the first [`Phase`]).
    Install(Phase, PhaseState),
    InstallDone(Result<(), String>),
    UninstallDone(Result<Box<UninstallReport>, String>),
    Launched(Result<(), String>),
    Plugin(crate::packages::Msg),
}

/// The window's handle on background work. Every job is a thread that ends by sending a message
/// and asking for a repaint; a status poller runs for the life of the window.
#[derive(Clone)]
pub struct Backend {
    pub mode: Mode,
    tx: Sender<Msg>,
    poke: Sender<()>,
    ctx: eframe::egui::Context,
}

impl Backend {
    pub fn start(mode: Mode, ctx: eframe::egui::Context) -> (Backend, Receiver<Msg>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (poke, poked) = std::sync::mpsc::channel::<()>();
        {
            let (tx, mode, ctx) = (tx.clone(), mode.clone(), ctx.clone());
            std::thread::Builder::new()
                .name("status".into())
                .spawn(move || {
                    loop {
                        if tx.send(Msg::Status(Box::new(read_status(&mode)))).is_err() {
                            return;
                        }
                        ctx.request_repaint();
                        match poked.recv_timeout(Duration::from_secs(3)) {
                            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                            Err(_) => return,
                        }
                        // Several pokes in a row: one refresh.
                        while poked.try_recv().is_ok() {}
                    }
                })
                .expect("cannot start a thread");
        }
        (
            Backend {
                mode,
                tx,
                poke,
                ctx,
            },
            rx,
        )
    }

    pub fn ctx_clone(&self) -> eframe::egui::Context {
        self.ctx.clone()
    }

    /// Read the status again now.
    pub fn refresh(&self) {
        let _ = self.poke.send(());
    }

    pub fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        self.ctx.request_repaint();
    }

    fn spawn(&self, name: &str, f: impl FnOnce(&Backend) + Send + 'static) {
        let me = self.clone();
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                f(&me);
                me.refresh();
            })
            .expect("cannot start a thread");
    }

    /// Install (or repair) PhoeCord, with a shortcut for `channel`.
    pub fn install(&self, channel: Option<Channel>) {
        self.spawn("install", move |b| {
            let r = (|| -> Result<()> {
                let paths = b.mode.paths();
                Config::load_or_init(&paths.config)?;
                let env = b.mode.env()?;
                if !b.mode.is_dry() && !env.proxy_exe.is_file() {
                    bail!("missing-binary");
                }
                let sys = b.mode.system();
                phoecord_installer::install::install_with_progress(
                    &env,
                    sys.as_ref(),
                    channel,
                    &mut |p, s| b.send(Msg::Install(p, s)),
                )?;
                Ok(())
            })();
            b.send(Msg::InstallDone(r.map_err(|e| format!("{e:#}"))));
        });
    }

    pub fn uninstall(&self) {
        self.spawn("uninstall", move |b| {
            let r = (|| -> Result<UninstallReport> {
                let env = b.mode.env()?;
                let sys = b.mode.system();
                phoecord_installer::uninstall::uninstall_all(&env, sys.as_ref())
            })();
            b.send(Msg::UninstallDone(
                r.map(Box::new).map_err(|e| format!("{e:#}")),
            ));
        });
    }

    /// Start `channel` through PhoeCord: `phoecord launch --channel <c>`, detached.
    pub fn launch(&self, channel: Channel) {
        self.spawn("launch", move |b| {
            let r = if b.mode.is_dry() {
                Ok(())
            } else {
                spawn_detached(&proxy_exe(), &["launch", "--channel", channel.name()])
            };
            b.send(Msg::Launched(r.map_err(|e| format!("{e:#}"))));
        });
    }

    pub fn plugin_job(
        &self,
        name: &str,
        f: impl FnOnce() -> crate::packages::Msg + Send + 'static,
    ) {
        self.spawn(name, move |b| b.send(Msg::Plugin(f())));
    }
}

/// Run `exe args` without a console window and without waiting for it.
pub fn spawn_detached(exe: &Path, args: &[&str]) -> Result<()> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    Ok(())
}

/// Open a folder or file in the system's file manager / default app.
pub fn open_path(p: &Path) -> Result<()> {
    let (exe, args): (&str, Vec<&std::ffi::OsStr>) = if cfg!(windows) {
        ("explorer", vec![p.as_os_str()])
    } else if cfg!(target_os = "macos") {
        ("open", vec![p.as_os_str()])
    } else {
        ("xdg-open", vec![p.as_os_str()])
    };
    std::process::Command::new(exe)
        .args(args)
        .spawn()
        .with_context(|| format!("opening {}", p.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dry(dir: &Path) -> Mode {
        Mode {
            sandbox: Some(dir.to_path_buf()),
            demo: true,
            open: None,
        }
    }

    #[test]
    fn dry_run_keeps_every_path_in_the_sandbox() {
        let t = tempfile::tempdir().unwrap();
        let m = dry(t.path());
        let env = m.env().unwrap();
        for p in [
            &env.data_dir,
            &env.home,
            &env.config_home,
            &env.start_menu,
            &env.applications_dir,
        ] {
            assert!(p.starts_with(t.path()), "{}", p.display());
        }
        assert!(m.paths().config.starts_with(t.path()));
    }

    #[test]
    fn dry_install_then_uninstall_round_trip() {
        let t = tempfile::tempdir().unwrap();
        let m = dry(t.path());
        let env = m.env().unwrap();
        let sys = m.system();
        assert!(!read_status(&m).installed);
        phoecord_installer::install::install_with(&env, sys.as_ref(), Some(Channel::Canary))
            .unwrap();
        let s = read_status(&m);
        assert!(s.installed);
        assert_eq!(s.trusted, Some(true));
        assert!(s.is_on(Channel::Canary));
        assert!(!s.needs_repair());
        let r = phoecord_installer::uninstall::uninstall_all(&env, sys.as_ref()).unwrap();
        assert!(r.is_clean());
        assert!(!read_status(&m).installed);
    }

    #[test]
    fn demo_needs_dry_run() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(!Mode::from_args(&args(&["x", "--demo"])).demo);
        assert!(Mode::from_args(&args(&["x", "--dry-run", "--demo"])).demo);
    }
}
