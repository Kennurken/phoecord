# Hello (TypeScript): the plugin template

The TypeScript twin of [`examples/hello-plugin`](../hello-plugin): it logs the message and channel id of every `MESSAGE_CREATE`, with a running count, and never logs the text. It is built into a WebAssembly component against [`wit/phoecord.wit`](../../wit/phoecord.wit) with [`componentize-js`](https://github.com/bytecodealliance/ComponentizeJS), so PhoeCord loads it like any WASM package.

## Build

Needs [Node.js](https://nodejs.org/) (LTS) and npm.

```bash
npm ci
npm run build
```

`npm run build` does three things:

1. `npm run types`: `jco guest-types` writes TypeScript types for the WIT world into `types/` (the `phoecord:plugin/*` modules: `host`, `types` and the exports you write);
2. `tsc` checks `src/` against them;
3. [`build.mjs`](build.mjs) bundles `src/plugin.ts` into one ES module (esbuild) and `componentize-js` turns it into `plugin.wasm`.

## Install

Copy `plugin.toml` and `plugin.wasm` into the `plugins/hello_plugin_ts/` data folder (`phoecord paths` shows it), then add to `config.toml`:

```toml
[plugins.hello_plugin_ts]
enabled = true
```

A running proxy loads it within 2 seconds. Press Ctrl+R in Discord so the Gateway is read for it.

## Writing your own

- **Exports.** A plugin exports `manifest()`, `wants(kind)` and `onEvent(event)` (WIT names in camelCase). `onEvent` returns `{ verdict, payload, extra }`: `verdict` is `{ tag: "pass" }`, `{ tag: "changed" }` (then `payload` is the new tree), `{ tag: "drop" }` or `{ tag: "reply", val: { status, body } }`.
- **Host calls.** Import them from `phoecord:plugin/host@1.0.0`: `log`, `setting`, `kvGet`, `rest`, `fetch`, `t`, `schedule`… A call that can fail returns its value or throws. List the rights you use in `plugin.toml` (`rights = ["kv", "rest", …]`) and the events in `events`.
- **Values.** Payloads are WIT `tree`s (an arena of nodes, root at `nodes[0]`). [`src/tree.ts`](src/tree.ts) reads them (`at`, `text`, `idAt`) and converts both ways (`toJs`, `fromJs`). 64-bit integers are `bigint`.
- **No WASI.** PhoeCord gives plugins no WASI, so `build.mjs` turns off `random`, `clocks`, `stdio`, `http` and `fetch-event`: no `Math.random`, `Date.now`, `console.log` or `fetch()`. Use the host's `log`, `fetch` and `schedule` instead.
- **Size and start time.** The component carries a whole JavaScript engine (StarlingMonkey): about 12 MB, compiled to native code each time PhoeCord loads it: about 17 s in a release build on a desktop PC (minutes in a debug build), against well under a second for a Rust plugin. PhoeCord does not cache compiled plugins yet. For plugins that run on every gateway event, Rust is the faster choice.

## Test

`cargo test -p phoecord-plugin-runtime --test hello_plugin_ts_wasm -- --ignored` builds this example, loads it as a package and checks the logs. It is ignored by default because it needs Node.js; the CI job `typescript-template` runs it.
