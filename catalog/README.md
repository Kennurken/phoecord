# Plugin catalog

`plugins.json` lists community WASM plugins that PhoeCord can install by id:

```
phoecord catalog              # what is listed, what is installed
phoecord install <id>         # shows what the plugin asks for, asks, installs
phoecord update [id]          # newer versions of what you installed from here
phoecord remove <id>
```

The installer window's Plugins tab has the same list under "Catalog". PhoeCord reads the file from the `main` branch of this repository at run time, so a merged entry is live at once; `PHOECORD_CATALOG_URL` points a copy of PhoeCord at a mirror.

## What the index guarantees

- Every download is checked against the `sha256` of its entry **before it is unpacked**. A different file is refused and nothing is installed.
- The unpacked package must have the entry's `id` and `version`, and its `plugin.toml` may not ask for a right the entry does not list. The user still sees the rights dialog (window) or the rights list and question (CLI).
- Updates are offered only for packages that were installed from the catalog.

The index does **not** make a plugin safe by itself: it makes it the plugin that was reviewed. The review is the pull request below.

## Adding or updating a plugin

1. Build your package: a `.zip` with `plugin.toml` and `plugin.wasm` (see [`docs/PLUGINS.md`](../docs/PLUGINS.md) and the SDK). Publish it as a **fixed release file** (for example a GitHub release asset), never a link to a branch: the hash must stay true.
2. Compute the hash: `sha256sum plugin.zip` (Windows: `Get-FileHash plugin.zip -Algorithm SHA256`), lower-case hex.
3. Open a pull request that changes only `catalog/plugins.json`: add your entry (entries are sorted by `id`) or raise `version`, `download` and `sha256` of yours.
4. Put the source link and the build steps in the PR text. A maintainer rebuilds the package, compares the hash, reads the code and the rights, then merges. Rights that are not needed, `rest` and `emit_send` (which act as the user on Discord), and downloads from places the author may change later are the usual reasons to ask for changes.

```json
{
  "id": "hello_wasm",
  "name": "Hello",
  "author": "Your name",
  "description": "One sentence on what it does.",
  "version": "1.0.0",
  "download": "https://github.com/you/hello/releases/download/v1.0.0/hello_wasm.zip",
  "sha256": "64 lower-case hex digits",
  "rights": ["notify"]
}
```

`rights` are the names from `plugin.toml` (`kv`, `notify`, `emit_send`, `emit_dispatch`, `guilds`, `fetch`, `rest`, `plugins`, `data`). [`plugins.schema.json`](plugins.schema.json) describes the file for editors; the real check is the `catalog_plugins_json_is_valid` test in `crates/phoecord-catalog` (it runs with `cargo test --workspace` in CI) which also checks sorting, unique ids, versions, `https://` links and known rights.

## Removing an entry

A plugin that turns out to be harmful is removed by a pull request that deletes its entry. Installed copies stay until the user removes them; tell users in the PR and in `docs/CHANGELOG.md` when that matters.
