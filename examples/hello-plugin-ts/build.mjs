// Bundle src/plugin.ts into one ES module and turn it into a WebAssembly component against
// ../../wit/phoecord.wit (world `plugin`). Output: plugin.wasm next to plugin.toml.
import { build } from "esbuild";
import { componentize } from "@bytecodealliance/componentize-js";
import { mkdir, writeFile } from "node:fs/promises";

await mkdir("dist", { recursive: true });
await build({
  entryPoints: ["src/plugin.ts"],
  bundle: true,
  format: "esm",
  // The host interface is provided by PhoeCord, not bundled.
  external: ["phoecord:*"],
  outfile: "dist/plugin.js",
});

const { component } = await componentize({
  sourcePath: "dist/plugin.js",
  witPath: "../../wit",
  worldName: "plugin",
  // PhoeCord gives plugins no WASI: with these off the component imports nothing but `host`.
  // So no `Math.random`, no `Date.now`, no `console.log` (use `log` from the host instead).
  disableFeatures: ["random", "clocks", "stdio", "http", "fetch-event"],
});
await writeFile("plugin.wasm", component);
console.log(`plugin.wasm: ${(component.length / 1048576).toFixed(1)} MB`);
