import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const bridge = JSON.parse(fs.readFileSync(path.join(root, "schemas/tauri-bridge-v1/bridge.schema.json"), "utf8"));
const payloads = JSON.parse(fs.readFileSync(path.join(root, "schemas/tauri-bridge-v1/payloads.json"), "utf8"));
const commands = bridge.properties.command.enum;
const queries = bridge.properties.query.enum;
const events = bridge.properties.event_type.enum;

// The bridge surface is what the Control Room calls, so it deserves the same drift discipline as the protocol
// crate. Before --check existed, the TypeScript surface could fall behind the contract with nothing noticing:
// verify.mjs checked that every bridge command had payload metadata, but never that the generated file
// matched. The Rust side already had byte-level drift detection and the TypeScript side did not, which is a
// parity gap rather than a design difference.
//
// --check re-derives the file and compares bytes without writing, which is what lets contract verification
// assert the generated TypeScript is current.
const ownership = (name) => payloads.commands?.[name]?.owner ?? payloads.queries?.[name]?.owner ?? null;

const ts = `// GENERATED FILE - DO NOT EDIT.
// Source: schemas/tauri-bridge-v1/bridge.schema.json
// Regenerate: npm run codegen:bridge
//
// Commands, queries and events are the machine-readable Tauri surface the Control Room calls, and each
// operation's owning service comes from payloads.json so the UI can route a call to its authority.

export const COMMANDS = ${JSON.stringify(commands, null, 2)} as const;
export const QUERIES = ${JSON.stringify(queries, null, 2)} as const;
export const EVENTS = ${JSON.stringify(events, null, 2)} as const;

export type CommandName = typeof COMMANDS[number];
export type QueryName = typeof QUERIES[number];
export type EventName = typeof EVENTS[number];

/** Owning application service per command, from payloads.json. */
export const COMMAND_OWNERS = {
${commands.map((c) => `  ${JSON.stringify(c)}: ${JSON.stringify(ownership(c))},`).join("\n")}
} as const;

/** Owning application service per query, from payloads.json. */
export const QUERY_OWNERS = {
${queries.map((q) => `  ${JSON.stringify(q)}: ${JSON.stringify(ownership(q))},`).join("\n")}
} as const;
`;

const target = path.join(root, "apps/desktop/src/generated/bridge.ts");

// The Rust surface is generated from the same three enums. It was checked in as a "GENERATED FILE - DO NOT
// EDIT" with no generator owning it and no check reading it, so it could drift from the contract (and from
// bridge.ts) with nothing noticing - the same parity gap bridge.ts had before its own --check existed. One
// generator now owns both, and --check compares both, so the two surfaces cannot disagree with the contract
// or with each other.
const rust = `// GENERATED FILE — DO NOT EDIT.
// Source: schemas/tauri-bridge-v1/bridge.schema.json + payloads.json + workspace.manifest.json
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandName { ${commands.join(", ")} }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryName { ${queries.join(", ")} }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventName { ${events.join(", ")} }
`;

const outputs = [
  { path: target, content: ts, label: "apps/desktop/src/generated/bridge.ts" },
  {
    path: path.join(root, "apps/desktop/src-tauri/src/generated/bridge.rs"),
    content: rust,
    label: "apps/desktop/src-tauri/src/generated/bridge.rs",
  },
];

if (process.argv.includes("--check")) {
  const stale = [];
  for (const { path: file, content, label } of outputs) {
    if (!fs.existsSync(file)) stale.push(`${label} is missing`);
    // Line-ending agnostic: git stores these files with LF while `core.autocrlf=true` may check them out with
    // CRLF on Windows, so a raw byte comparison would call them stale on a checkout whose content is identical.
    else if (fs.readFileSync(file, "utf8").replace(/\r\n/g, "\n") !== content.replace(/\r\n/g, "\n")) {
      stale.push(`${label} is stale`);
    }
  }
  if (stale.length) {
    console.error(
      "generated bridge surface is out of date:\n  - " + stale.join("\n  - ") +
      "\nRun: npm run codegen:bridge"
    );
    process.exit(1);
  }
  console.log("generated bridge surface is up to date");
  process.exit(0);
}

fs.mkdirSync(path.join(root, "apps/desktop/src/generated"), { recursive: true });
fs.mkdirSync(path.join(root, "apps/desktop/src-tauri/src/generated"), { recursive: true });
for (const { path: file, content } of outputs) fs.writeFileSync(file, content);
console.log(
  `Generated bridge.ts and bridge.rs (${commands.length} commands, ${queries.length} queries, ${events.length} events)`
);