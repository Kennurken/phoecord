# GPUI lifecycle of the native Discord client (Project Phoenix)

Reverse-engineering notes on how Phoenix starts its GPUI (Mana) UI, which view wraps which, and where PhoeCord's UI hooks (stage 2.3) would attach. **Addresses, vtable offsets, `TypeId`s and the exact binary/build this was read from are not published here**: they are reverse-engineering detail about a specific shipped build and are kept in the team's private recon notes instead (consistent with [`docs/AGENTS.md`](../AGENTS.md), "Never commit … disassembly"). What follows is the architecture, which doesn't need them.

## 1. GPUI initialization chain and main window

- `WindowsPlatform::new` registers the window class and creates a hidden event window; `gpui_platform::current_platform` wraps it and `gpui::Application::new` installs the platform and builds an `Rc<AppCell>`.
- A Discord-specific `Application` builder calls `Application::new()`, then stores the client's `AssetSource` and `HttpClient` on the `App`.
- `desktop_main` parses CLI flags (`--print-voice-engine`, `--capture-self-test`, `--startup-self-test`, `--krisp-self-test`, `--phoenix-setup-quit`), reads a couple of env vars (`MANA_NATIVE_POPUPS`, `DISCORD_DESKTOP_BENCH_CHANNEL`), builds the `Application`, binds a core runtime to it, and calls `Application::run`.
- `Application::run` calls `Platform::run` with a boxed closure that becomes the `on_finish_launching` callback.
- `on_finish_launching` registers a handful of global stores and actions on `&mut App`, calls `boot_window`, and on success starts background tasks via `cx.spawn`.
- `boot_window` builds `WindowOptions` and calls `App::open_window::<gpui_base::root::Root>`.
- `open_window::<Root>` reserves an entity slot, allocates the `Root` view, creates the native `gpui::window::Window`, a `Frame` entity and the root UI container, then builds and binds the `Root`'s `AnyView` with its render function as the window's root view.

### Summary

```
desktop_main
 ├─ Application builder
 │    └─ Application::new
 │         ├─ current_platform -> WindowsPlatform::new
 │         └─ Application::with_platform -> Rc<AppCell>
 ├─ bind core runtime
 └─ Application::run
      └─ Platform::run -> on_finish_launching
           ├─ register stores / actions
           ├─ boot_window
           │    └─ open_window::<Root>
           │         ├─ Window
           │         ├─ Frame, scene
           │         └─ AnyView<Root>
           └─ cx.spawn background tasks
```

## 2. Injection points for phoecord (architecture, no addresses)

1. **`App` / global store initialization**: hook the exit of the Discord `Application` builder (a ready `Application` / `Rc<AppCell>` on the main UI thread before the window opens), or inside `on_finish_launching` after the stores are registered.
2. **Render pipeline and UI overlays**: the `AnyView` trampoline for `gpui_base::root::Root` gives access to rendering the root tree every frame.
3. **Client component tree**: the `Frame` and main-app-scene constructors. Next step: analyse the concrete views and store subscriptions.

## 3. View hierarchy and GPUI primitives

The UI is built from GPUI components (`AnyView`, `AnyElement`). Each view has a type-name string, a `TypeId` and a vtable whose render slot points to the `Render::render` wrapper.

Root view chain:

`gpui_base::root::Root` -> `discord_desktop_platform::frame::Frame` -> `discord_desktop::app::Splash` -> `discord_desktop::app::DiscordApp` -> router / `Workspace`

### GPUI primitives (by name only)

A central `AnyView` registration/creation dispatcher (takes the type name, size, alignment and a pointer to the render vtable); `gpui::div()`; `Div::child` and `Div::children`; `IntoElement::into_any_element`; a function that renders the global overlay layer (modals, tooltips, menus).

### Client views

Named views in the root chain: `discord_desktop::app::DiscordApp` (app root view; its `render` has the implementation `DiscordApp::render`), `discord_desktop::app::Splash` (loading screen), `discord_desktop_platform::frame::Frame` (main window frame), `discord_desktop_platform::frame::PopoutFrame` (popped-out windows), `discord_desktop::workspace::Region` (workspace sections), `discord_desktop::nav::menus::MenuView` (context and dropdown menus), `discord_desktop::nav::FnView` (view backed by a Rust closure), `discord_desktop::modal::NativeBody` (native modal container).

### `DiscordApp` internals and gates

- An access-gate check: on denial it flags the app state and sends an analytics event (`"desktop_access_gate_blocked"`); `DiscordApp::render` checks the gate's state and renders a "Build expired" banner when it isn't passed.
- A start-route resolver that returns `"/login"` or a stored path depending on the app's auth state.
- A one-shot transition from `Splash` to `DiscordApp` once ready.
- A function that mounts the active router page / `Workspace` inside `DiscordApp::render`.

## 4. UI hook architecture for phoecord

1. **Global UI injection (toasts, overlays, loader settings):** hook the exit of `DiscordApp::render`. Once the root `Div` is built, call `Div::child` to add a custom `AnyElement` / `FnView` on top of the app tree.
2. **Embedding in the interface (settings tabs, sidebar, panels):** analyse the router-mounting function and `discord_desktop::workspace::Region` to inject custom sections and tabs.
3. **Context menu patching (the `PatchContextMenu` analogue):** hook `discord_desktop::nav::menus::MenuView::render` to add plugin items to right-click menus.

## Open questions

- Gate struct layout and where `gate` comes from in `DiscordApp::render`.
- Whether `Div::child` can be called on the finished root element at the render exit, or the hook must wrap the returned `AnyElement`.
- Signature and calling convention of the `AnyView` render trampoline are not recorded here.
- The store/action registration functions called from `on_finish_launching` are not decoded here.
- The build and hash of the analysed binary are kept in the private recon notes, not here.
