// Values cross the WASM boundary as a `tree` (see `wit/phoecord.wit`): an arena where
// `nodes[0]` is the root and a list, tuple or map refers to its children by index.
// These helpers read a tree and build one from plain JavaScript values.

import type { Node, Tree } from "phoecord:plugin/types@1.0.0";

/** The node at `path` (map keys), or `undefined`. `at(tree, ["d", "id"])`. */
export function at(tree: Tree, path: string[]): Node | undefined {
  let index = 0;
  for (const key of path) {
    const node = tree.nodes[index];
    if (node?.tag !== "map") return undefined;
    const entry = node.val.find(([k]) => text(tree.nodes[k]) === key);
    if (!entry) return undefined;
    index = entry[1];
  }
  return tree.nodes[index];
}

/** A string node's text (strings, atoms and binaries all count). */
export function text(node: Node | undefined): string | undefined {
  switch (node?.tag) {
    case "string":
    case "atom":
      return node.val;
    case "bin":
    case "charlist":
      return new TextDecoder().decode(node.val);
    default:
      return undefined;
  }
}

/** A snowflake at `path`: Discord sends ids as strings, the ETF gateway as integers. */
export function idAt(tree: Tree, path: string[]): bigint | undefined {
  const node = at(tree, path);
  if (node?.tag === "int") return node.val;
  if (node?.tag === "big") return (node.val[0] << 64n) | node.val[1];
  const s = text(node);
  return s !== undefined && /^\d+$/.test(s) ? BigInt(s) : undefined;
}

/** A whole tree as a plain value: maps become objects, integers `bigint`. */
export function toJs(tree: Tree, index = 0): unknown {
  const node = tree.nodes[index];
  switch (node.tag) {
    case "null":
      return null;
    case "bool":
    case "int":
    case "float":
    case "string":
    case "atom":
      return node.val;
    case "big":
      return (node.val[0] << 64n) | node.val[1];
    case "bin":
    case "charlist":
      return new TextDecoder().decode(node.val);
    case "list":
    case "tuple":
      return Array.from(node.val, (i) => toJs(tree, i));
    case "map":
      return Object.fromEntries(
        node.val.map(([k, v]) => [String(toJs(tree, k)), toJs(tree, v)]),
      );
  }
}

/** A plain value as a tree: objects become maps, `bigint` and whole numbers integers. */
export function fromJs(value: unknown): Tree {
  const nodes: Node[] = [];
  const add = (v: unknown): number => {
    const index = nodes.length;
    nodes.push({ tag: "null" });
    let node: Node;
    if (v === null || v === undefined) node = { tag: "null" };
    else if (typeof v === "boolean") node = { tag: "bool", val: v };
    else if (typeof v === "bigint") node = { tag: "int", val: v };
    else if (typeof v === "number")
      node = Number.isInteger(v) ? { tag: "int", val: BigInt(v) } : { tag: "float", val: v };
    else if (typeof v === "string") node = { tag: "string", val: v };
    else if (Array.isArray(v)) node = { tag: "list", val: Uint32Array.from(v.map(add)) };
    else
      node = {
        tag: "map",
        val: Object.entries(v as Record<string, unknown>).map(([k, x]) => [add(k), add(x)]),
      };
    nodes[index] = node;
    return index;
  };
  add(value);
  return { nodes };
}
