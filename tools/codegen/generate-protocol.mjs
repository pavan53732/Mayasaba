import fs from "node:fs";
import path from "node:path";

// Generates crates/protocol/src/generated/machines.rs from the machine-readable contract.
//
// Input is deliberately transition-types.json:machines[] plus registry.json, NOT transitions[]. The
// transitions array is a hand-maintained ledger of 132 records whose 16 fields include one distinct
// authorization, one idempotency_behavior and one transaction_boundary, wrapping the four-field fact that
// actually matters: source, target, event, command. Reading it would mean emitting 132 copies of the same
// boilerplate and then un-repeating it. machines[] is 5 KB of structural intent - owner, states, spine,
// branches - and registry.json carries the enums, event emitters and command registrations. That is the
// input a generator should read, and DEC-042 limitation (3) records the diagnosis.
//
// Consequence worth stating plainly: this generator therefore CANNOT emit per-edge transition functions.
// branches carries no command, no event and no owner, so the generated surface stops at machine shape,
// state sets, declared spine and the declared event emitters. Per-edge data still has to be read from
// transitions[], and pretending otherwise would mean inventing data the declaration does not contain.

const root = process.cwd();
const readJson = (p) => JSON.parse(fs.readFileSync(path.join(root, p), "utf8"));

const transitionTypes = readJson("schemas/mcf-v2/transition-types.json");
const registry = readJson("schemas/mcf-v2/registry.json");
const manifest = readJson("workspace.manifest.json");

const machines = transitionTypes.machines;
const emitters = registry.event_emitters ?? {};
const eventTypes = readJson("schemas/mcf-v2/event-types.schema.json").enum;
const agentTypes = readJson("schemas/mcf-v2/identity.schema.json").properties.agent_type.enum.filter((x) => x !== null);

// Rust identifiers: contract names are SCREAMING_SNAKE, Rust variants want CamelCase.
const camel = (s) => s.toLowerCase().split(/[_\s]+/).map((p) => p.charAt(0).toUpperCase() + p.slice(1)).join("");
const pascal = (s) => camel(s);
const rustDoc = (s) => s.replace(/\*\//g, "*\\/").replace(/\r?\n/g, " ").trim();
const variantDocs = (names, describe) => {
  const described = names.map((n) => [n, describe(n)]);
  const width = Math.max(...described.map(([n]) => n.length));
  return described.map(([n, d]) => `    /// ${d}${" ".repeat(width - n.length)} ${n}`).join("\n");
};

const machineNames = Object.keys(machines).sort();
const machineDocs = {};
for (const [name, def] of Object.entries(machines)) {
  const owner = def.owner ?? null;
  machineDocs[name] = { owner, states: def.states.length, branches: (def.branches ?? []).length };
}

const commandNames = Object.keys(registry.transition_commands?.commands ?? {}).sort();
const serviceEvents = Object.entries(emitters)
  .filter(([, v]) => v && v.kind === "service")
  .map(([k]) => k)
  .sort();
const transitionEvents = Object.entries(emitters)
  .filter(([, v]) => v && v.kind === "transition")
  .map(([k]) => k)
  .sort();

const out = `// GENERATED FILE - DO NOT EDIT.
// Source: schemas/mcf-v2/transition-types.json (machines[]), schemas/mcf-v2/registry.json
// Regenerate: npm run codegen:protocol
//
// Generated from structural intent only. Per-edge transition data is deliberately not emitted; see the
// generator header for why transitions[] is the wrong input.

/// Every state machine Mayasaba coordinates, from transition-types.json:machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Machine {
${variantDocs(machineNames, (n) => machineDocs[n].owner ? `owned by ${machineDocs[n].owner}` : "no application service")}
}

impl Machine {
    /// Every machine, in the contract's sorted order.
    pub const ALL: &'static [Machine] = &[${machineNames.map((n) => `Machine::${pascal(n)}`).join(", ")}];

    /// The contract's identifier for this machine.
    pub fn id(self) -> &'static str {
        match self {
${machineNames.map((n) => `            Machine::${pascal(n)} => ${JSON.stringify(n)},`).join("\n")}
        }
    }

    /// The machine a contract identifier names.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
${machineNames.map((n) => `            ${JSON.stringify(n)} => Some(Machine::${pascal(n)}),`).join("\n")}
            _ => None,
        }
    }
}

impl std::fmt::Display for Machine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// States of the \`message_delivery\` machine.
///
/// Each machine declares its ordered path as \`spine\` and its legal states as \`states\`; the states set may
/// hold terminal and branch states that are not on the ordered path (DEC-038, DEC-042).
pub mod message_delivery_states {
${machines.message_delivery.states.map((s) => `    pub const ${s}: &str = ${JSON.stringify(s)};`).join("\n")}
}

/// The ordered path of the \`message_delivery\` machine.
pub const MESSAGE_DELIVERY_SPINE: &[&str] = &[${machines.message_delivery.spine.map((s) => JSON.stringify(s)).join(", ")}];

/// Declared off-spine edges of the \`message_delivery\` machine, blessed or awaiting review.
///
/// A branch listed here was declared legal by a decision; the gate does not and cannot prove the
/// declaration is correct (DEC-042 limitation 1).
pub const MESSAGE_DELIVERY_BRANCHES: &[&str] = &[${(machines.message_delivery.branches ?? []).map((s) => JSON.stringify(s)).join(", ")}];

/// Declared off-spine edges of the \`message_delivery\` machine that are legal but not yet reviewed.
pub const MESSAGE_DELIVERY_UNREVIEWED_BRANCHES: &[&str] = &[${(machines.message_delivery.unreviewed_branches ?? []).map((s) => JSON.stringify(s)).join(", ")}];

/// State count per machine, from machines[].states.
pub fn state_count(machine: Machine) -> usize {
    match machine {
${machineNames.map((n) => `        Machine::${pascal(n)} => ${machines[n].states.length},`).join("\n")}
    }
}

/// Declared off-spine edge count per machine, from machines[].branches.
pub fn branch_count(machine: Machine) -> usize {
    match machine {
${machineNames.map((n) => `        Machine::${pascal(n)} => ${(machines[n].branches ?? []).length},`).join("\n")}
    }
}

/// The ordered path per machine, from machines[].spine.
pub fn spine(machine: Machine) -> &'static [&'static str] {
    match machine {
${machineNames.map((n) => `        Machine::${pascal(n)} => &[${machines[n].spine.map((s) => JSON.stringify(s)).join(", ")}],`).join("\n")}
    }
}

/// The legal state set per machine, from machines[].states.
pub fn states(machine: Machine) -> &'static [&'static str] {
    match machine {
${machineNames.map((n) => `        Machine::${pascal(n)} => &[${machines[n].states.map((s) => JSON.stringify(s)).join(", ")}],`).join("\n")}
    }
}

/// Declared off-spine edges per machine, from machines[].branches.
pub fn branches(machine: Machine) -> &'static [&'static str] {
    match machine {
${machineNames.map((n) => `        Machine::${pascal(n)} => &[${(machines[n].branches ?? []).map((s) => JSON.stringify(s)).join(", ")}],`).join("\n")}
    }
}

/// Declared off-spine edges that are legal but not yet reviewed, per machine.
pub fn unreviewed_branches(machine: Machine) -> &'static [&'static str] {
    match machine {
${machineNames.map((n) => `        Machine::${pascal(n)} => &[${(machines[n].unreviewed_branches ?? []).map((s) => JSON.stringify(s)).join(", ")}],`).join("\n")}
    }
}

/// Every canonical MCF event type.
pub const EVENT_TYPES: &[&str] = &[${eventTypes.map((e) => JSON.stringify(e)).join(", ")}];

/// The three adapters Mayasaba coordinates at runtime (DEC-029).
pub const AGENT_TYPES: &[&str] = &[${agentTypes.map((a) => JSON.stringify(a)).join(", ")}];

/// Every registered transition command.
pub const TRANSITION_COMMANDS: &[&str] = &[${commandNames.map((c) => JSON.stringify(c)).join(", ")}];

/// Events emitted by a state machine transition, from registry.json:event_emitters.
pub const TRANSITION_EMITTED_EVENTS: &[&str] = &[${transitionEvents.map((e) => JSON.stringify(e)).join(", ")}];

/// Events emitted by a service rather than a state transition, from registry.json:event_emitters.
pub const SERVICE_EMITTED_EVENTS: &[&str] = &[${serviceEvents.map((e) => JSON.stringify(e)).join(", ")}];

/// Crate owning each machine, from machines[].owner.
pub fn owner_crate(machine: Machine) -> Option<&'static str> {
    match machine {
${machineNames.map((n) => `        Machine::${pascal(n)} => ${machines[n].owner ? JSON.stringify(machines[n].owner) : "None"},`).join("\n")}
    }
}
`;

const dest = path.join(root, "crates/protocol/src/generated");
const target = path.join(dest, "machines.rs");

// --check verifies the committed file matches what the contract currently implies, without writing. This is
// what lets contract verification assert the generated crate is in sync, which matters because no Rust
// toolchain is configured in this environment: the bytes can be proven current even though they cannot be
// proven to compile here.
if (process.argv.includes("--check")) {
  if (!fs.existsSync(target)) {
    console.error("Generated file is missing: crates/protocol/src/generated/machines.rs");
    process.exit(1);
  }
  const current = fs.readFileSync(target, "utf8");
  if (current !== out) {
    console.error(
      "crates/protocol/src/generated/machines.rs is stale relative to the contract.\n" +
      "Run: npm run codegen:protocol"
    );
    process.exit(1);
  }
  console.log("crates/protocol/src/generated/machines.rs is up to date");
  process.exit(0);
}

fs.mkdirSync(dest, { recursive: true });
fs.writeFileSync(target, out);
console.log(
  `Generated crates/protocol/src/generated/machines.rs ` +
  `(${machineNames.length} machines, ${eventTypes.length} events, ${commandNames.length} commands, ` +
  `${transitionEvents.length} transition-emitted and ${serviceEvents.length} service-emitted events)`
);