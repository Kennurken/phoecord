// Hello (TypeScript): logs every `MESSAGE_CREATE`. It logs ids and a running count, never the
// text. The same plugin as the Rust example `examples/hello-plugin`.

import { log } from "phoecord:plugin/host@1.0.0";
import type { ManifestInfo, Kind, PluginEvent, PluginOutcome } from "phoecord:plugin/plugin@1.0.0";
import { idAt } from "./tree";

let seen = 0;

const pass: PluginOutcome = { verdict: { tag: "pass" }, payload: undefined, extra: [] };

export function manifest(): ManifestInfo {
  return {
    id: "hello_plugin_ts",
    name: "Hello (TypeScript example)",
    version: "0.1.0",
    description: "Logs the ids of every new message",
    authors: ["matvee"],
    vencord: undefined,
    layers: ["network"],
    api: 1,
  };
}

export function wants(kind: Kind): boolean {
  return kind.tag === "dispatch" && kind.val === "MESSAGE_CREATE";
}

export function onEvent(event: PluginEvent): PluginOutcome {
  if (event.tag !== "dispatch" || event.val.name !== "MESSAGE_CREATE") return pass;
  // The payload is the whole gateway frame; the message is under `d`.
  const id = (key: string) => idAt(event.val.payload, ["d", key]) ?? 0n;
  seen += 1;
  log(`MESSAGE_CREATE #${seen}: message ${id("id")} in channel ${id("channel_id")}`);
  return pass;
}
