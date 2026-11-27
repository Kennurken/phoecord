//! Plugin entries in the client's native context menus (tasks 2.3.5, 2.3.11).
//!
//! A plugin declares its entries in [`Manifest::menu`](phoecord_core::Manifest) (`[[menu]]` in a
//! package's `plugin.toml`): an id, the key of its label in the plugin's own strings and the menu
//! it goes in ([`MenuKind`]). The Phoenix loader's menu hooks ask for [`Engine::menu_items`]
//! (directly in an in-process launch, over the loader IPC through the proxy otherwise), show each
//! one in its menu, and a click comes back as [`Engine::menu_action`], which runs that menu's
//! `UiEvent` for the declaring plugin only.

use crate::host::ProxyHost;
use crate::{Effects, Engine};
use phoecord_core::{MenuKind, UiEvent};
use serde::{Deserialize, Serialize};

/// One entry a running plugin adds to a context menu, its label already in the user's language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    /// The declaring plugin's id.
    pub plugin: String,
    /// The entry's [`MenuEntry::id`](phoecord_core::MenuEntry).
    pub entry: String,
    /// The entry's label, looked up in the plugin's strings (`Host::t` rules: the user's language,
    /// then English, then the key itself).
    pub label: String,
    /// The menu it goes in ([`MenuKind::name`]).
    pub menu: String,
}

/// What a click on a menu entry is about: the ids of the menu it was in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuClick {
    Message {
        channel_id: u64,
        message_id: u64,
    },
    User {
        user_id: u64,
        guild_id: Option<u64>,
        channel_id: Option<u64>,
    },
    Channel {
        channel_id: u64,
        guild_id: Option<u64>,
    },
    Server {
        guild_id: u64,
    },
}

impl MenuClick {
    pub fn kind(&self) -> MenuKind {
        match self {
            MenuClick::Message { .. } => MenuKind::Message,
            MenuClick::User { .. } => MenuKind::User,
            MenuClick::Channel { .. } => MenuKind::Channel,
            MenuClick::Server { .. } => MenuKind::Server,
        }
    }

    /// The event the declaring plugin gets for entry `entry`.
    pub fn event<'a>(&self, entry: &'a str) -> UiEvent<'a> {
        match *self {
            MenuClick::Message {
                channel_id,
                message_id,
            } => UiEvent::MenuAction {
                entry,
                channel_id,
                message_id,
            },
            MenuClick::User {
                user_id,
                guild_id,
                channel_id,
            } => UiEvent::UserMenuAction {
                entry,
                user_id,
                guild_id,
                channel_id,
            },
            MenuClick::Channel {
                channel_id,
                guild_id,
            } => UiEvent::ChannelMenuAction {
                entry,
                channel_id,
                guild_id,
            },
            MenuClick::Server { guild_id } => UiEvent::ServerMenuAction { entry, guild_id },
        }
    }
}

impl Engine {
    /// `plugin`'s string `key` in the user's language (`Host::t` outside a plugin call), the key
    /// itself when the plugin lacks it. The settings tab labels settings with it (2.3.3).
    pub fn plugin_text(&self, plugin: &str, key: &str) -> String {
        let lang = self.shared.language(&self.cfg());
        self.shared.strings.t(plugin, &lang, key, &[])
    }

    /// The menu entries of every plugin that runs now (enabled and not crashed), in chain order.
    pub fn menu_items(&self) -> Vec<MenuItem> {
        let cfg = self.cfg();
        let running = self.chain.enabled_ids();
        let lang = self.shared.language(&cfg);
        self.chain
            .manifests()
            .into_iter()
            .filter(|m| running.contains(&m.id))
            .flat_map(|m| {
                let plugin = m.id;
                m.menu
                    .into_iter()
                    .map(|e| MenuItem {
                        label: self.shared.strings.t(&plugin, &lang, &e.label, &[]),
                        plugin: plugin.clone(),
                        entry: e.id,
                        menu: e.menu.name().to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// A click on `plugin`'s menu entry `entry`: that menu's `UiEvent` for that plugin alone. What
    /// it queues is returned for the transport to deliver at once. An entry the plugin does not
    /// declare in that menu, or a plugin that is off, runs nothing (`Err` says why, for the log).
    pub fn menu_action(
        &self,
        plugin: &str,
        entry: &str,
        click: MenuClick,
    ) -> Result<Effects, String> {
        let declared = self
            .chain
            .manifests()
            .into_iter()
            .find(|m| m.id == plugin)
            .is_some_and(|m| {
                m.menu
                    .iter()
                    .any(|e| e.id == entry && e.menu == click.kind())
            });
        if !declared {
            return Err(format!(
                "{plugin} declares no {} menu entry {entry}",
                click.kind().name()
            ));
        }
        let cfg = self.cfg();
        let mut host = ProxyHost::new(&self.shared, &cfg);
        if self
            .chain
            .ui_to(&mut host, plugin, click.event(entry))
            .is_none()
        {
            return Err(format!(
                "{plugin} is off or does not listen to menu actions"
            ));
        }
        Ok(self.finish(host))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::host::Shared;
    use phoecord_core::{
        Event, EventKind, Host, Manifest, MenuEntry, MenuKind, Plugin, Val, Verdict,
    };
    use phoecord_plugin_runtime::Chain;
    use std::sync::{Arc, Mutex};

    /// Declares two menu entries and records the clicks it gets; answers each with a local payload.
    struct Clicks(Manifest, Arc<Mutex<Vec<String>>>);

    impl Plugin for Clicks {
        fn manifest(&self) -> &Manifest {
            &self.0
        }
        fn wants(&self, e: &EventKind) -> bool {
            matches!(e, EventKind::Ui { .. })
        }
        fn on_event(&self, host: &mut dyn Host, e: Event<'_>) -> Verdict {
            if let Event::Ui(UiEvent::MenuAction {
                entry,
                channel_id,
                message_id,
            }) = e
            {
                self.1
                    .lock()
                    .unwrap()
                    .push(format!("{entry} {channel_id} {message_id}"));
                host.emit_dispatch(Val::Str(entry.to_string()));
            }
            Verdict::Pass
        }
    }

    fn engine(ids: &[&str]) -> (Engine, Arc<Mutex<Vec<String>>>) {
        let dir = std::env::temp_dir().join(format!(
            "phoecord-test-menu-{}-{}",
            std::process::id(),
            ids.join("-")
        ));
        let log = Arc::new(Mutex::new(Vec::new()));
        let plugins: Vec<Box<dyn Plugin>> = ids
            .iter()
            .map(|id| {
                let mut m = Manifest::new(*id, *id, "0", "");
                m.menu = vec![
                    MenuEntry::new("copy_raw", "menu-copy-raw"),
                    MenuEntry::new("quote", "menu-quote"),
                ];
                Box::new(Clicks(m, log.clone())) as Box<dyn Plugin>
            })
            .collect();
        let e = Engine::new(Chain::new(plugins), Shared::new(dir), Config::default());
        // Test plugins are not in the config; turn them on by hand.
        e.chain.set_enabled(ids.iter().map(|s| s.to_string()));
        (e, log)
    }

    fn msg(channel_id: u64, message_id: u64) -> MenuClick {
        MenuClick::Message {
            channel_id,
            message_id,
        }
    }

    #[test]
    fn running_plugins_list_their_entries_in_chain_order() {
        let (e, _) = engine(&["a", "b"]);
        let items = e.menu_items();
        let got: Vec<(&str, &str)> = items
            .iter()
            .map(|i| (i.plugin.as_str(), i.entry.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("a", "copy_raw"),
                ("a", "quote"),
                ("b", "copy_raw"),
                ("b", "quote")
            ]
        );
        // No strings bundle for these plugins: the label falls back to its key.
        assert_eq!(items[0].label, "menu-copy-raw");

        e.chain.set_enabled(["b".to_string()]);
        assert!(e.menu_items().iter().all(|i| i.plugin == "b"));
    }

    #[test]
    fn a_user_menu_entry_gets_the_user_event() {
        let dir =
            std::env::temp_dir().join(format!("phoecord-test-menu-user-{}", std::process::id()));
        let log = Arc::new(Mutex::new(Vec::new()));
        struct Users(Manifest, Arc<Mutex<Vec<String>>>);
        impl Plugin for Users {
            fn manifest(&self) -> &Manifest {
                &self.0
            }
            fn wants(&self, e: &EventKind) -> bool {
                matches!(e, EventKind::Ui { .. })
            }
            fn on_event(&self, _: &mut dyn Host, e: Event<'_>) -> Verdict {
                if let Event::Ui(ev) = e {
                    self.1
                        .lock()
                        .unwrap()
                        .push(format!("{} {:?}", ev.name(), ev.to_val()));
                }
                Verdict::Pass
            }
        }
        let mut m = Manifest::new("u", "u", "0", "");
        m.menu = vec![MenuEntry::new("avatar", "menu-avatar").in_menu(MenuKind::User)];
        let e = Engine::new(
            Chain::new(vec![Box::new(Users(m, log.clone()))]),
            Shared::new(dir),
            Config::default(),
        );
        e.chain.set_enabled(["u".to_string()]);
        assert_eq!(e.menu_items()[0].menu, "user");
        let click = MenuClick::User {
            user_id: 7,
            guild_id: Some(10),
            channel_id: None,
        };
        e.menu_action("u", "avatar", click).unwrap();
        assert!(log.lock().unwrap()[0].starts_with("user_menu_action "));
        assert!(
            e.menu_action("u", "avatar", msg(1, 2)).is_err(),
            "wrong menu"
        );
    }

    #[test]
    fn a_plugin_with_menu_entries_keeps_the_gateway_decoded() {
        // Its click answers with local payloads, which only a decoded connection can carry.
        let (e, _) = engine(&["a"]);
        assert!(e.wants_gateway());
        e.chain.set_enabled(Vec::<String>::new());
        assert!(!e.wants_gateway());
    }

    #[test]
    fn a_click_reaches_the_declaring_plugin_only() {
        let (e, log) = engine(&["a", "b"]);
        let effects = e.menu_action("b", "quote", msg(11, 50)).unwrap();
        assert_eq!(*log.lock().unwrap(), ["quote 11 50"]);
        assert_eq!(effects.locals, vec![Val::Str("quote".into())]);

        assert!(e.menu_action("b", "nope", msg(11, 50)).is_err());
        assert!(e.menu_action("zz", "quote", msg(11, 50)).is_err());
        let user = MenuClick::User {
            user_id: 7,
            guild_id: None,
            channel_id: None,
        };
        assert!(
            e.menu_action("b", "quote", user).is_err(),
            "quote is a message-menu entry"
        );
        e.chain.set_enabled(["a".to_string()]);
        assert!(
            e.menu_action("b", "quote", msg(11, 50)).is_err(),
            "b is off"
        );
        assert_eq!(log.lock().unwrap().len(), 1);
    }
}
