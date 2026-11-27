//! Plugin entries in the client's native message context menu (task 2.3.5).
//!
//! A plugin declares its entries in [`Manifest::menu`](phoecord_core::Manifest) (`[[menu]]` in a
//! package's `plugin.toml`): an id and the key of its label in the plugin's own strings. The Phoenix
//! loader's menu hook asks for [`Engine::menu_items`] (directly in an in-process launch, over the
//! loader IPC through the proxy otherwise), shows each one in the menu, and a click comes back as
//! [`Engine::menu_action`], which runs `UiEvent::MenuAction` for the declaring plugin only.

use crate::host::ProxyHost;
use crate::{Effects, Engine};
use phoecord_core::UiEvent;
use serde::{Deserialize, Serialize};

/// One entry a running plugin adds to the message context menu, its label already in the user's
/// language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    /// The declaring plugin's id.
    pub plugin: String,
    /// The entry's [`MenuEntry::id`](phoecord_core::MenuEntry).
    pub entry: String,
    /// The entry's label, looked up in the plugin's strings (`Host::t` rules: the user's language,
    /// then English, then the key itself).
    pub label: String,
}

impl Engine {
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
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// A click on `plugin`'s menu entry `entry` in the menu of message `message_id` in channel
    /// `channel_id`: `UiEvent::MenuAction` for that plugin alone. What it queues is returned for
    /// the transport to deliver at once. An entry the plugin does not declare, or a plugin that is
    /// off, runs nothing (`Err` says why, for the log).
    pub fn menu_action(
        &self,
        plugin: &str,
        entry: &str,
        channel_id: u64,
        message_id: u64,
    ) -> Result<Effects, String> {
        let declared = self
            .chain
            .manifests()
            .into_iter()
            .find(|m| m.id == plugin)
            .is_some_and(|m| m.menu.iter().any(|e| e.id == entry));
        if !declared {
            return Err(format!("{plugin} declares no menu entry {entry}"));
        }
        let cfg = self.cfg();
        let mut host = ProxyHost::new(&self.shared, &cfg);
        let event = UiEvent::MenuAction {
            entry,
            channel_id,
            message_id,
        };
        if self.chain.ui_to(&mut host, plugin, event).is_none() {
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
    use phoecord_core::{Event, EventKind, Host, Manifest, MenuEntry, Plugin, Val, Verdict};
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
        let effects = e.menu_action("b", "quote", 11, 50).unwrap();
        assert_eq!(*log.lock().unwrap(), ["quote 11 50"]);
        assert_eq!(effects.locals, vec![Val::Str("quote".into())]);

        assert!(e.menu_action("b", "nope", 11, 50).is_err());
        assert!(e.menu_action("zz", "quote", 11, 50).is_err());
        e.chain.set_enabled(["a".to_string()]);
        assert!(e.menu_action("b", "quote", 11, 50).is_err(), "b is off");
        assert_eq!(log.lock().unwrap().len(), 1);
    }
}
