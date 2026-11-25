# GPUI lifecycle of the native Discord client (Project Phoenix)

Reverse-engineering notes: x86_64 Windows, ImageBase `0x140000000`. Addresses are virtual addresses (VA). Function names are reconstructed, not symbols (except string markers found in the binary). The build/hash these addresses belong to is not recorded; treat them as valid for that one binary only.

## 1. GPUI initialization chain and main window

### Platform

| Address | Function | What it does |
|---|---|---|
| `0x14f118dd0` | `WindowsPlatform::new` | Registers the window class `L"Zed::PlatformWindow"` and creates the hidden event window via `CreateWindowExW(0x80, ...)`. |
| `0x14eb8f1e0` | `gpui_platform::current_platform` | Unwraps `WindowsPlatform::new()` (error `"failed to initialize Windows platform"`) and allocates `Rc<WindowsPlatform>` of `0x1b8` bytes. |

### Application

| Address | Function | What it does |
|---|---|---|
| `0x14eb8f1c0` | `gpui::Application::new` | Calls `current_platform(0)`, installs the platform vtable `&PTR_LAB_1570c8ae8`, tail-calls `LAB_1487712e0` (`Application::with_platform`). That checks the main-thread TLS (`"must construct App on main thread"`) and creates `Rc<AppCell>`. |
| `0x144576ba0` | Discord `Application` builder | Calls `Application::new()`, gets `&mut App` via `FUN_14876fd90(app + 0x10)`, stores the `AssetSource` (`app + 0x4f8`) and `HttpClient` (`app + 0x508`), both `Arc<dyn ...>` with vtable `&PTR_FUN_154a27680`. Returns the configured `Application`. |

### `desktop_main` — `0x14464d7d3`

- Parses CLI flags: `--print-voice-engine`, `--capture-self-test`, `--startup-self-test`, `--krisp-self-test`, `--phoenix-setup-quit`.
- Reads env: `MANA_NATIVE_POPUPS`, `DISCORD_DESKTOP_BENCH_CHANNEL`.
- Default window size `1280x800` (`0x44a00000`, `0x44480000` as f32).
- Initializes the core runtime (`FUN_14965c770`).
- `0x144650be4`: `FUN_144576ba0()` -> `app`.
- `0x144650c40`: `FUN_145241efe(&app, core_runtime)` binds the core runtime to `App`.
- `0x144650d51`: `FUN_14524202a(app, closure_env)` is `Application::run`.

### `gpui::Application::run` — `0x14524202a`

- Takes `Rc<dyn Platform>` at `app + 0x308`.
- Packs `0x160` bytes of closure environment plus `Rc<AppCell>` into a `0x168`-byte heap allocation (`Box<dyn FnOnce()>`).
- Calls `Platform::run` (vtable slot `+0x38`) with closure vtable `&PTR_LAB_154a27700`:
  - `drop_in_place` = `LAB_145294890`
  - `size` = `0x168`
  - `call_once` (`0x154a27718`) = `LAB_1453fda30`

### `on_finish_launching` `call_once` — `0x1453fda30`

- Logs `"mana initwindow"`.
- Registers global stores and actions on `&mut App`: `FUN_148346940`, `FUN_148347ed0`, `FUN_1483473c0`, `FUN_1454c0527`, `FUN_144587001`, `FUN_145a78935`, `FUN_1452704b7`.
- Calls `FUN_144da1720` (`boot_window`).
- On success: logs `"window"` and starts background tasks via `FUN_148109b60` (`cx.spawn` at `app + 0x348`).

### `boot_window` — `0x144da1720`

- Creates `Rc<RefCell<Option<Entity<Frame>>>>` (`FUN_14087ecb2`).
- Builds `WindowOptions` (`FUN_144da0ec0`).
- Calls `FUN_144ca7128` (`open_window`); checks the error `"boot window: no frame"`.

### `App::open_window::<gpui_base::root::Root>` — `0x144ca7128`

- Reserves an entity slot (`FUN_1493832e0`) and allocates `0x38f8` bytes for `gpui_base::root::Root` (`TypeId` = `0xb6df3784c1838927_df5395e92d6b9dca`).
- Creates the native `gpui::window::Window` (`FUN_148af84a0`).
- `FUN_14532ddcd` creates `Entity<Frame>`; `FUN_14530db02` builds the root UI container inside `Root`.
- `0x144ca77f7`: builds the `AnyView` for `gpui_base::root::Root` with render function `&LAB_146216150` and binds it as the window's root view via `FUN_14832f3c0`.

### Summary

```
desktop_main (0x14464d7d3)
 ├─ FUN_144576ba0 (Application builder)
 │    └─ Application::new (0x14eb8f1c0)
 │         ├─ current_platform (0x14eb8f1e0) -> WindowsPlatform::new (0x14f118dd0)
 │         └─ Application::with_platform (LAB_1487712e0) -> Rc<AppCell>
 ├─ FUN_145241efe (bind core runtime)
 └─ Application::run (0x14524202a)
      └─ Platform::run -> on_finish_launching (0x1453fda30)
           ├─ register stores / actions
           ├─ boot_window (0x144da1720)
           │    └─ open_window::<Root> (0x144ca7128)
           │         ├─ Window (FUN_148af84a0)
           │         ├─ Frame (FUN_14532ddcd), scene (FUN_14530db02)
           │         └─ AnyView<Root>, render = 0x146216150
           └─ cx.spawn background tasks (FUN_148109b60)
```

## 2. Injection points for phoecord

1. **`App` / global store initialization**: hook the exit of `0x144576ba0` (a ready `Application` / `Rc<AppCell>` on the main UI thread before the window opens), or inside `0x1453fda30` after the stores are registered.
2. **Render pipeline and UI overlays**: the `AnyView` trampoline for `gpui_base::root::Root` at `0x146216150` (`LAB_146216150`) gives access to rendering the root tree every frame.
3. **Client component tree**: constructors `FUN_14532ddcd` (`Frame`) and `FUN_14530db02` (main app scene). Next step: analyse the concrete views and store subscriptions.

## 3. View hierarchy and GPUI primitives

The UI is built from GPUI components (`AnyView`, `AnyElement`). Each view has a type-name string, a 128-bit `TypeId` and a vtable whose slot `+0x18` points to the `Render::render` wrapper.

Root view chain:

`gpui_base::root::Root` -> `discord_desktop_platform::frame::Frame` -> `discord_desktop::app::Splash` -> `discord_desktop::app::DiscordApp` -> router / `Workspace`

### GPUI primitives

| Address | Function | Role |
|---|---|---|
| `0x148784dd0` | `FUN_148784dd0` | Central `AnyView` registration/creation dispatcher (takes the type name `&str`, size, alignment and a pointer to the render vtable). |
| `0x148457500` | `FUN_148457500` | `gpui::div()` constructor (takes `&'static core::panic::Location`). |
| `0x14842bc60` | `FUN_14842bc60(out, parent, child)` | `Div::child` (append one `impl IntoElement`). |
| `0x14842bed0` | `FUN_14842bed0(out, parent, Option<child>)` | `Div::children`. |
| `0x14841da20` | `thunk_FUN_14841da20` | `IntoElement::into_any_element`. |
| `0x148bb5b40` | `FUN_148bb5b40` | Renders the global overlay layer (modals, tooltips, menus). |

### Client views

`vtable + 0x18` = `Render::render`.

| View | Trampoline | Vtable | Notes |
|---|---|---|---|
| `discord_desktop::app::DiscordApp` (app root view) | `0x145185ce0` | `0x154a122b0` | `Render::render` wrapper `0x1451fde00`; implementation `DiscordApp::render` at `0x144a44984` (`FUN_144a44984(out_element, window, discord_app_ptr, cx)`). |
| `discord_desktop::app::Splash` (loading screen) | `0x145185f20` | `0x154a12490` | `Render::render` wrapper `0x1451fde80`. |
| `discord_desktop_platform::frame::Frame` (main window frame) | `0x145185860` | `0x154a11ab0` | |
| `discord_desktop_platform::frame::PopoutFrame` (popped-out windows) | `0x145185620` | `0x154a11a58` | |
| `discord_desktop::workspace::Region` (workspace sections) | `0x145186ca0` | `0x154a12cf0` | `+0x18` -> `0x154a12d08`. |
| `discord_desktop::nav::menus::MenuView` (context and dropdown menus) | `0x14518a960` | `0x154a156e0` | `+0x18` -> `0x154a156f8`. |
| `discord_desktop::nav::FnView` (view backed by a Rust closure) | `0x1451865e0` | `0x154a125a0` | |
| `discord_desktop::modal::NativeBody` (native modal container) | `0x145186820` | `0x154a125e8` | |

### `DiscordApp` internals and gates

- `0x1449b622d` (`FUN_1449b622d`): Access Gate check. On denial sets `*(int *)(app + 0x60) = 1` and sends `"desktop_access_gate_blocked"`. `DiscordApp::render` (`0x144a44984`) checks `*(int *)(gate + 0x40) == 2`; otherwise it renders the `"Build expired"` banner.
- `0x1449b8496` (`FUN_1449b8496`): start-route resolver. When `*(int *)(app + 0x78) == 2` it returns `"/login"` or the path stored at `app + 0x80`.
- `0x1449b4cf1` (`FUN_1449b4cf1`): one-shot transition from `Splash` to `DiscordApp` (sets `app + 0x26b = 1`).
- `0x1449e1a6d` (`FUN_1449e1a6d`): mounts the active router page / `Workspace` inside `DiscordApp::render`.

## 4. UI hook architecture for phoecord

1. **Global UI injection (toasts, overlays, loader settings):** hook the exit of `DiscordApp::render` (`0x144a44984`). Once the root `Div` is built (held in `uVar5`), call `Div::child` (`0x14842bc60`) to add a custom `AnyElement` / `FnView` (`0x154a125a0`) on top of the app tree.
2. **Embedding in the interface (settings tabs, sidebar, panels):** analyse `FUN_1449e1a6d` and `discord_desktop::workspace::Region` (`0x154a12cf0`) to inject custom sections and tabs.
3. **Context menu patching (the `PatchContextMenu` analogue):** hook `discord_desktop::nav::menus::MenuView::render` (pointer at `0x154a156f8`) to add plugin items to right-click menus.

## Open questions

- Gate struct layout behind `*(int *)(gate + 0x40)`: where `gate` comes from in `DiscordApp::render`.
- Whether `Div::child` can be called on the finished root element at the render exit, or the hook must wrap the returned `AnyElement`.

- Signature and calling convention of `LAB_146216150` (the `AnyView` render function) are not recorded.
- The store/action registration functions called from `0x1453fda30` are not decoded.
- The build and hash of the analysed binary are not recorded.
