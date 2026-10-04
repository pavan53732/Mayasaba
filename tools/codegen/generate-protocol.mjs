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
  // The doc comment and the variant are separate lines. Putting the name at the end of the comment
  // produces an enum with doc comments and no variants, which is exactly what happened the first time
  // this was generated: rustc rejected it with E0585 and no other check in the repository could see it.
  // The variant is the idiomatic Rust CamelCase form; the contract's snake_case identifier is what
  // id() returns. Emitting the raw snake_case name here would not compile, because every accessor below
  // refers to the PascalCase form - a mismatch that produced 121 errors the first time this was built.
  return described.map(([n, d]) => `    /// ${d}\n    ${pascal(n)},`).join("\n");
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
${machineNames.map((n) => `        Machine::${pascal(n)} => ${machines[n].owner ? `Some(${JSON.stringify(machines[n].owner)})` : "None"},`).join("\n")}
    }
}
`;

const envelopeSchema = readJson("schemas/mcf-v2/envelope.schema.json");
const identitySchema = readJson("schemas/mcf-v2/identity.schema.json");
const enumsSchema = readJson("schemas/mcf-v2/enums.schema.json");
const messageTypes = readJson("schemas/mcf-v2/message-types.schema.json").enum;
const strArray = (values) => `&[${values.map((v) => JSON.stringify(v)).join(", ")}]`;

// The envelope's allowed vocabulary is derived, never hand-copied. If the contract adds a channel, a phase or a
// message type, this file changes on the next generation run and the Rust validator starts accepting it. A
// hand-written list would drift silently, which is the failure this repository keeps removing elsewhere.
const materialConditional = envelopeSchema.allOf?.[0] ?? {};
const materialActionTypes = materialConditional.if?.properties?.message_type?.enum ?? [];
// The conditional's `required` is deduped defensively, so a duplicate in the schema would NOT surface as a diff
// in this generated file. That is why repeated entries in a canonical schema's `required` array are detected in
// the contract gate instead, which checks every canonical schema for them.
const materialRequired = [...new Set(materialConditional.then?.required ?? [])];
const envelopeOptional = Object.keys(envelopeSchema.properties).filter((k) => !envelopeSchema.required.includes(k));

// The envelope's nested objects are contracts in their own right: authorization_context and security each
// declare their own `required` and `additionalProperties:false`, and one identity shape serves both the sender
// and every recipient. Hand-copying those field lists into the validator is how the two drifted before:
// authorization_context.required names seven fields and the validator checked six, so an envelope could
// authorize a material action without ever stating its required capabilities.
const authContextSchema = envelopeSchema.properties.authorization_context;
const securitySchema = envelopeSchema.properties.security;
const authContextRequired = authContextSchema.required ?? [];
const authContextFields = Object.keys(authContextSchema.properties ?? {});
const securityRequired = securitySchema.required ?? [];
const securityFields = Object.keys(securitySchema.properties ?? {});
const identityRequired = identitySchema.required ?? [];
const identityFields = Object.keys(identitySchema.properties ?? {});

// schema_version's `pattern` is a real assertion, unlike `format`, which JSON Schema treats as annotation only.
// The validator implements the pattern structurally - numeric MINOR and PATCH, and the MAJOR the contract pins.
// Deriving the MAJOR here keeps one source of truth; an unrecognised pattern shape stops generation loudly
// rather than letting the validator accept a set the contract does not describe.
const schemaVersionPattern = envelopeSchema.properties.schema_version.pattern;
const versionShape = /^\^(\d+)\\\.\\d\+\\\.\\d\+\$$/.exec(schemaVersionPattern);
if (!versionShape) {
  throw new Error(
    `envelope.schema.json:schema_version.pattern is ${JSON.stringify(schemaVersionPattern)}, which is not the ` +
      "`^<major>\\.\\d+\\.\\d+$` shape implemented by is_schema_version() in crates/protocol/src/envelope.rs. " +
      "Update that function to match the contract, and this derivation with it."
  );
}
const schemaVersionMajor = versionShape[1];

// A field -> JSON type table, so the validator never hand-copies which envelope field is a string, an integer or
// a boolean. Only the JSON types this contract actually uses are named; an unmapped type is a generation error
// rather than a silently unchecked field. Fields with no `type` at all - a `$ref`, a bare `enum`, a bare `const` -
// are absent from the table by construction, and the hand-written rule named for each one enforces its vocabulary.
const kindNames = { string: "Str", integer: "Int", number: "Num", boolean: "Bool", object: "Obj", array: "Arr", null: "Null" };
const typedFields = [];
for (const [name, spec] of Object.entries(envelopeSchema.properties)) {
  const declared = Array.isArray(spec.type) ? spec.type : spec.type === undefined ? null : [spec.type];
  if (declared === null) continue;
  const kinds = declared.map((t) => kindNames[t]);
  if (kinds.some((k) => k === undefined)) {
    throw new Error(
      `envelope.schema.json:properties.${name}.type declares ${JSON.stringify(declared)}, which this generator ` +
        "does not map. Extend kindNames here and holds_type() in crates/protocol/src/envelope.rs together."
    );
  }
  typedFields.push([name, kinds]);
}

const envelopeRs = `// GENERATED FILE - DO NOT EDIT.
// Source: schemas/mcf-v2/envelope.schema.json, identity.schema.json, enums.schema.json
// Regenerate: npm run codegen:protocol
//
// The envelope's vocabulary is generated from the contract so the Rust validator and the schema cannot
// disagree about what a legal envelope is. The validation logic itself is hand-written in envelope.rs,
// because a generic JSON Schema evaluator would be a dependency this repository deliberately does not have.

/// The only protocol version this build speaks.
pub const PROTOCOL_VERSION: &str = ${JSON.stringify(envelopeSchema.properties.protocol_version.const)};

/// Every legal channel.
pub const CHANNELS: &[&str] = ${strArray(envelopeSchema.properties.channel.enum)};

/// Every legal lifecycle phase, including UNSCOPED for messages outside a project phase.
pub const PHASES: &[&str] = ${strArray(envelopeSchema.properties.phase.enum)};

/// Every legal message type.
pub const MESSAGE_TYPES: &[&str] = ${strArray(messageTypes)};

/// Message types that authorize a material action and therefore require the full authorization context.
pub const MATERIAL_ACTION_MESSAGE_TYPES: &[&str] = ${strArray(materialActionTypes)};

/// Fields every envelope must carry.
pub const REQUIRED_FIELDS: &[&str] = ${strArray(envelopeSchema.required)};

/// Fields an envelope may carry. The schema sets additionalProperties:false, so anything else is rejected.
pub const OPTIONAL_FIELDS: &[&str] = ${strArray(envelopeOptional)};

/// The JSON type(s) each typed envelope field may hold, from the contract's own \`type\` keyword.
///
/// A field absent from this table declares no single \`type\` in the contract - it is a \`$ref\`, a bare \`enum\`
/// or a bare \`const\` - so its vocabulary is enforced by the hand-written rule named for it instead. The names
/// are resolved by holds_type() in envelope.rs.
pub const FIELD_TYPES: &[(&str, &[&str])] = &[${typedFields.map(([n, ks]) => `(${JSON.stringify(n)}, &[${ks.map((k) => JSON.stringify(k)).join(", ")}])`).join(", ")}];

/// Extra fields a material-action envelope must carry beyond the base set. The contract's conditional narrows
/// each of these to a non-nullable type in \`then.properties\`, so presence alone is not enough: a null value
/// does not satisfy it.
pub const MATERIAL_REQUIRED_FIELDS: &[&str] = ${strArray(materialRequired)};

/// Fields every authorization_context must carry, from the contract's own nested \`required\`.
pub const AUTHORIZATION_CONTEXT_REQUIRED_FIELDS: &[&str] = ${strArray(authContextRequired)};

/// Fields an authorization_context may carry. The contract sets additionalProperties:false on it.
pub const AUTHORIZATION_CONTEXT_FIELDS: &[&str] = ${strArray(authContextFields)};

/// Fields every security block must carry.
pub const SECURITY_REQUIRED_FIELDS: &[&str] = ${strArray(securityRequired)};

/// Fields a security block may carry. The contract sets additionalProperties:false on it.
pub const SECURITY_FIELDS: &[&str] = ${strArray(securityFields)};

/// Fields every actor identity must carry, applied to the sender and to every recipient alike.
pub const IDENTITY_REQUIRED_FIELDS: &[&str] = ${strArray(identityRequired)};

/// Fields an actor identity may carry. The contract sets additionalProperties:false on it.
pub const IDENTITY_FIELDS: &[&str] = ${strArray(identityFields)};

/// The MAJOR that schema_version must carry, derived from the contract's pattern.
pub const SCHEMA_VERSION_MAJOR: &str = ${JSON.stringify(schemaVersionMajor)};

/// Every legal delivery priority.
pub const PRIORITIES: &[&str] = ${strArray(enumsSchema.$defs.priority.enum)};

/// Every legal security classification.
pub const CLASSIFICATIONS: &[&str] = ${strArray(envelopeSchema.properties.security.properties.classification.enum)};

/// Every legal actor kind.
pub const ACTOR_TYPES: &[&str] = ${strArray(identitySchema.properties.actor_type.enum)};

/// Every legal agent type, from the three supported adapters (DEC-029).
pub const AGENT_TYPES: &[&str] = ${strArray(identitySchema.properties.agent_type.enum.filter((x) => x !== null))};
`;

const dest = path.join(root, "crates/protocol/src/generated");
const outputs = [
  { path: path.join(dest, "machines.rs"), content: out, label: "machines.rs" },
  { path: path.join(dest, "envelope.rs"), content: envelopeRs, label: "envelope.rs" },
];

// --check verifies the committed files match what the contract currently implies, without writing. This is
// what lets contract verification assert the generated crate is in sync, which matters because no Rust
// toolchain was configured when this was written: the bytes can be proven current even though they cannot be
// proven to compile at that point.
if (process.argv.includes("--check")) {
  const stale = [];
  for (const { path: file, content, label } of outputs) {
    if (!fs.existsSync(file)) stale.push(`${label} is missing`);
    else if (fs.readFileSync(file, "utf8") !== content) stale.push(`${label} is stale`);
  }
  if (stale.length) {
    console.error(
      "crates/protocol/src/generated is out of date:\n  - " + stale.join("\n  - ") +
      "\nRun: npm run codegen:protocol"
    );
    process.exit(1);
  }
  console.log("crates/protocol/src/generated is up to date");
  process.exit(0);
}

fs.mkdirSync(dest, { recursive: true });
for (const { path: file, content } of outputs) fs.writeFileSync(file, content);
console.log(
  `Generated machines.rs (${machineNames.length} machines, ${eventTypes.length} events, ${commandNames.length} commands) ` +
  `and envelope.rs (${messageTypes.length} message types, ${materialActionTypes.length} material-action types)`
);