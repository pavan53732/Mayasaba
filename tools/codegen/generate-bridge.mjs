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

if (process.argv.includes("--check")) {
  if (!fs.existsSync(target)) {
    console.error("Generated file is missing: apps/desktop/src/generated/bridge.ts");
    process.exit(1);
  }
  // Line-ending agnostic, for the same reason and with the same reproduction as the protocol generator: git
// stores this file with LF while `core.autocrlf=true` checks it out with CRLF on Windows, so a raw byte
// comparison called it stale on a fresh checkout whose content was identical.
if (fs.readFileSync(target, "utf8").replace(/\r\n/g, "\n") !== ts.replace(/\r\n/g, "\n")) {
    console.error(
      "apps/desktop/src/generated/bridge.ts is stale relative to the contract.\n" +
      "Run: npm run codegen:bridge"
    );
    process.exit(1);
  }
  console.log("apps/desktop/src/generated/bridge.ts is up to date");
  process.exit(0);
}

fs.mkdirSync(path.join(root, "apps/desktop/src/generated"), { recursive: true });
fs.writeFileSync(target, ts);
console.log(
  `Generated bridge.ts (${commands.length} commands, ${queries.length} queries, ${events.length} events)`
);