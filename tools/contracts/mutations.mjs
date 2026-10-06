#!/usr/bin/env node
/**
 * Mutation tests for the contract gate and for the wire-shape conformance test.
 *
 * A check that has never been shown to fail is an assertion, not a check. `tools/contracts/verify.mjs` and the
 * shell's `wire_shape_tests` both exist to catch a specific class of drift, and the only evidence that they can
 * is that each one, when the drift is deliberately reintroduced, fails and names it. Those proofs were
 * originally a throwaway script run by hand; this file is that script made durable, because a proof nobody can
 * re-run is a claim rather than a test.
 *
 * Each mutation is applied to the working tree, the relevant check is run, and the file is restored. A mutation
 * is a PASS only when the check fails AND its output names the specific disagreement the mutation introduced —
 * "exited non-zero" alone would accept a crash, a typo in the anchor, or an unrelated failure. Controls are the
 * reverse: the check must stay green, because a check that fires on prose or on a description would be turned
 * off rather than fixed.
 *
 * ## Why this is safe to run
 *
 * It refuses to start unless every tracked file is unmodified (`git status --porcelain` shows nothing but
 * untracked entries), so the only changes it can discard are its own, and it restores every file it touches
 * with `git checkout -- <file>` — which is why that precondition exists rather than being a courtesy. It
 * restores in a `finally`, on SIGINT, and on an uncaught exception, and it verifies at the end that the tree is
 * clean and HEAD is unchanged. If it is ever interrupted in a way that defeats all of that, the recovery is:
 *
 *     git checkout -- .
 *
 * which discards uncommitted changes, so it is the right command here and the wrong command in a tree with
 * work in it. Nothing in this file writes to the database, the network, or anywhere outside the listed files.
 *
 * ## Running it
 *
 *     npm run verify:contracts:mutations            # every mutation
 *     node tools/contracts/mutations.mjs --filter dec056   # only mutations whose id contains "dec056"
 *
 * It is deliberately NOT part of `npm run verify:contracts` and NOT wired to the pre-commit hook: it mutates
 * the tree, and the Rust mutations each pay a recompile. It is a tool to run when a check changes, not on every
 * commit.
 */

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..", "..");

const GATE = "gate";
const RUST = "rust";
// A rule that only `crates/core` enforces cannot be proven by the desktop check, which never compiles core's
// `#[cfg(test)]` modules. Without this the proof would have to be weakened to something the desktop check
// happens to observe, which would test a different rule.
const CORE = "core";
// The frontend tests are a check the harness could not previously use, so a rule only they hold could not be
// proven. The file list is read from `apps/desktop/package.json` rather than repeated here, because a second
// copy of "which tests exist" is exactly the drift this tool exists to catch. `npm` itself is not invoked:
// it is `npm.cmd` on Windows, and `spawnSync` without a shell cannot resolve it - the same reason the gate
// check calls node directly.
const UI = "ui";
// The historical integrity audit. It is the only check here that reads Git history rather than the tree, and the
// only one that can see text a later edit destroyed without leaving a structural trace, so the rule it holds
// cannot be proven by any other check. Like the frontend tests, it is invoked as `node` on a script rather than
// through npm, which is `npm.cmd` on Windows.
const INTEGRITY = "integrity";

const MAIN = "apps/desktop/src-tauri/src/main.rs";
const BRIDGE_TS = "apps/desktop/src/intake/bridge.ts";
const APP_TSX = "apps/desktop/src/App.tsx";
const REGISTRY = "schemas/error-v1/registry.json";
const VALIDATION_RS = "crates/workspace/src/validation.rs";
const BUS_ERROR_RS = "crates/bus/src/error.rs";
const BUS_POLICIES = "schemas/mcf-v2/bus-policies.json";
const BUS_POLICY_RS = "crates/bus/src/policy.rs";
const GENERATED_RS = "apps/desktop/src-tauri/src/generated/bridge.rs";
const GENERATED_TS = "apps/desktop/src/generated/bridge.ts";
const PAYLOAD_TYPES = "schemas/tauri-bridge-v1/payload-types.json";
const TRACEABILITY = "docs/TRACEABILITY.md";
const DECISION_REGISTER = "docs/DECISION-REGISTER.md";
const DATA_MODEL = "docs/DATA-MODEL.md";
const MANIFEST = "workspace.manifest.json";
const PAYLOADS = "schemas/tauri-bridge-v1/payloads.json";
const FAILURE_SCHEMA = "schemas/validation-v1/failure.schema.json";
const FAILURE_CLASS_POLICIES = "schemas/validation-v1/failure-class-policies.json";
const RECOVERY_SCHEMA = "schemas/recovery-v1/recovery.schema.json";
const REPAIR_POLICIES = "schemas/validation-v1/repair-policies.json";

// -----------------------------------------------------------------------------------------------------------
// The mutations.
//
// `expect` entries are substrings that must all appear in the check's output. They are quoted from the check's
// own messages rather than paraphrased, so a message that is reworded without being reconsidered fails here and
// forces the mutation to be re-read rather than silently matched by something vaguer.
// -----------------------------------------------------------------------------------------------------------
const MUTATIONS = [
  // --- DEC-053: the two-way bridge gate, which compares operation NAMES between the contract and both sides.
  {
    id: "dec053-a",
    what: "main.rs: register a handler the contract does not declare",
    check: GATE,
    edits: [
      {
        file: MAIN,
        find: "fn main() {",
        replace:
          '#[tauri::command(rename_all = "snake_case")]\nfn ghost_undeclared() -> u32 {\n    0\n}\n\nfn main() {',
      },
      {
        file: MAIN,
        find: "        .invoke_handler(tauri::generate_handler![\n            create_project,",
        replace:
          "        .invoke_handler(tauri::generate_handler![\n            ghost_undeclared,\n            create_project,",
      },
    ],
    expect: [
      "registers handler ghost_undeclared, which the bridge contract declares as neither a command nor a query",
    ],
  },
  {
    id: "dec053-b",
    what: "main.rs: a command function is left defined but dropped from generate_handler![...]",
    check: GATE,
    edits: [{ file: MAIN, find: "            validate_workspace,\n", replace: "" }],
    expect: [
      "defines #[tauri::command] fn validate_workspace but never registers it in generate_handler![...]",
    ],
  },
  {
    id: "dec053-c",
    what: "main.rs: the #[tauri::command] attribute is deleted from a registered handler",
    check: GATE,
    edits: [
      {
        file: MAIN,
        find: '#[tauri::command(rename_all = "snake_case")]\nfn create_project(',
        replace: "fn create_project(",
      },
    ],
    expect: ["registers create_project in generate_handler![...] but defines no #[tauri::command] fn create_project"],
  },
  {
    id: "dec053-d",
    what: "main.rs: the generate_handler![...] list no longer parses",
    check: GATE,
    edits: [{ file: MAIN, find: "tauri::generate_handler![", replace: "tauri::generate_handler!(" }],
    expect: ["Could not find a generate_handler![...] list"],
  },
  {
    id: "dec053-e",
    what: "frontend: a transport call names an operation the contract does not declare",
    check: GATE,
    edits: [{ file: BRIDGE_TS, append: '\ntransport("ghost_undeclared_op", {});\n' }],
    expect: [
      'calls transport("ghost_undeclared_op"), which the bridge contract declares as neither a command nor a query',
    ],
  },
  {
    id: "dec053-f",
    what: "frontend: a transport call names a declared operation that no handler registers",
    check: GATE,
    edits: [{ file: BRIDGE_TS, append: '\ntransport("get_project", {});\n' }],
    expect: ["registers no handler of that name, so the declared query cannot succeed"],
  },
  {
    id: "dec053-g",
    what: "frontend: a raw invoke(...) outside the boundary file, and a transport name that is not a literal",
    check: GATE,
    edits: [
      { file: APP_TSX, append: '\ninvoke("list_projects", {});\n' },
      { file: BRIDGE_TS, append: "\ntransport(dynamicName, {});\n" },
    ],
    expect: [
      "calls invoke(...) directly instead of going through the single transport boundary",
      "calls transport(...) with an operation name that is not a string literal",
    ],
  },
  {
    id: "dec053-h",
    what: "generated surface: both bridge files are edited without regenerating",
    check: GATE,
    edits: [
      { file: GENERATED_RS, append: "\n// mutation: hand-edited without regenerating\n" },
      { file: GENERATED_TS, append: "\n// mutation: hand-edited without regenerating\n" },
    ],
    expect: ["the generated bridge surface is stale or missing."],
  },

  // --- DEC-055: the error vocabulary, which compares the registry with the protocol's enums and with the code
  // that actually produces codes.
  {
    id: "dec055-a",
    what: "registry.json: a registered code states no meaning",
    check: GATE,
    edits: [
      {
        file: REGISTRY,
        find: '"meaning": "A message, payload or document does not conform to the schema declared for it."',
        replace: '"meaning": ""',
      },
    ],
    expect: ["declares no meaning, so the code is a name nothing downstream can act on"],
  },
  {
    id: "dec055-b",
    what: "registry.json: an mcf_code is not a member of the protocol's closed code enum",
    check: GATE,
    edits: [
      { file: REGISTRY, find: '"mcf_code": "MCF_SCHEMA_INVALID"', replace: '"mcf_code": "MCF_NOT_A_REAL_CODE"' },
    ],
    expect: ["is not a member of the MCF error code enum"],
  },
  {
    id: "dec055-c",
    what: "registry.json: two registry codes claim the same MCF code",
    check: GATE,
    edits: [{ file: REGISTRY, find: '"mcf_code": "MCF_TIMEOUT"', replace: '"mcf_code": "MCF_CANCELED"' }],
    expect: ["is already declared by", "one MCF code maps to one registry code"],
  },
  {
    id: "dec055-d",
    what: "frontend: the transport boundary produces a code the registry does not declare",
    check: GATE,
    edits: [{ file: BRIDGE_TS, find: '"TRANSPORT_FAILURE"', replace: '"TRANSPORT_FAULT"', all: true }],
    expect: ["which schemas/error-v1/registry.json does not register"],
  },
  {
    id: "dec055-e",
    what: "main.rs: a rejection mapping produces a code the registry does not declare",
    check: GATE,
    edits: [{ file: MAIN, find: '=> "INTAKE_NOT_IMPLEMENTED"', replace: '=> "INTAKE_MISSING"' }],
    expect: ["which schemas/error-v1/registry.json does not register"],
  },
  {
    id: "dec055-f",
    what: "crates/workspace: the error-code mapping this check scans is renamed away",
    check: GATE,
    edits: [
      {
        file: VALIDATION_RS,
        // Both mappings in the file, because the gate reads all of them: renaming one leaves the other to be
        // found, and the mutation would report the check as broken rather than prove it fails when the mapping
        // is gone.
        find: "pub fn code(self)",
        replace: "pub fn rejection_code(self)",
        all: true,
      },
    ],
    expect: ["could not find the error-code mapping this check scans"],
  },
  {
    id: "dec055-g",
    what: "crates/bus: the second error-code mapping this check scans is renamed away",
    check: GATE,
    edits: [{ file: BUS_ERROR_RS, find: "pub fn code(&self)", replace: "pub fn registry_code(&self)" }],
    expect: ["could not find the error-code mapping this check scans"],
  },
  {
    id: "dec055-h",
    what: "crates/bus: the mapping produces a code the registry does not declare",
    check: GATE,
    edits: [
      {
        file: BUS_ERROR_RS,
        // Anchored on the arm rather than on the code string: two arms legitimately produce
        // DUPLICATE_CONFLICT, and an anchor matching twice is not a mutation.
        find: 'BusError::SequenceConflict { .. } => "DUPLICATE_CONFLICT"',
        replace: 'BusError::SequenceConflict { .. } => "SEQUENCE_COLLISION"',
      },
    ],
    expect: ["which schemas/error-v1/registry.json does not register"],
  },

  // --- DEC-066: the gate reads the Rust for the delivery edges it advances through, so an edge the machine does
  // not declare cannot be reached by writing it in the code.
  {
    id: "dec066-a",
    what: "crates/storage: a requeue advances RETRYING -> PROCESSED, which the message_delivery machine does not declare",
    check: GATE,
    edits: [
      {
        file: "crates/storage/src/lib.rs",
        find: 'advance_in(tx, message_id, "RETRYING", "QUEUED", "a requeue")?;',
        replace: 'advance_in(tx, message_id, "RETRYING", "PROCESSED", "a requeue")?;',
      },
    ],
    expect: ["advances RETRYING->PROCESSED, which the message_delivery machine does not declare"],
  },  // --- DEC-058: the bus's retry/dispatch policy, against the machine it terminates through and against the
  // crate's shipped defaults.
  {
    id: "dec060-a",
    what: "crates/storage: the due query serves the lanes in an order the contract does not declare",
    check: GATE,
    edits: [
      {
        file: "crates/storage/src/lib.rs",
        find: "WHEN 'EMERGENCY_CONTROL' THEN 0\n                        WHEN 'SYNCHRONIZATION' THEN 1",
        replace: "WHEN 'SYNCHRONIZATION' THEN 0\n                        WHEN 'EMERGENCY_CONTROL' THEN 1",
      },
    ],
    expect: ["ranks lanes", "serves the wrong traffic first"],
  },
  {
    id: "dec059-a",
    what: "crates/bus: the inbound duplicate arm produces a code the registry does not declare",
    check: GATE,
    edits: [
      {
        file: BUS_ERROR_RS,
        find: 'BusError::DuplicateMessage { .. } => "DUPLICATE_CONFLICT"',
        replace: 'BusError::DuplicateMessage { .. } => "DUPLICATE_MESSAGE"',
      },
    ],
    expect: ["which schemas/error-v1/registry.json does not register"],
  },
  {
    id: "dec058-a",
    what: "bus-policies.json: the declared termination path is not an edge the message_delivery machine has",
    check: GATE,
    edits: [
      {
        file: BUS_POLICIES,
        find: '"on_attempt_budget_exhausted":"QUEUED_TO_EXPIRED"',
        replace: '"on_attempt_budget_exhausted":"QUEUED_TO_DEAD_LETTER"',
      },
    ],
    expect: ["terminal.on_attempt_budget_exhausted", "any other value needs a decision record"],
  },
  {
    id: "dec058-b",
    what: "crates/bus: the shipped default stops matching the policy file",
    check: GATE,
    edits: [{ file: BUS_POLICY_RS, find: "pub const MAX_ATTEMPTS: i64 = 5;", replace: "pub const MAX_ATTEMPTS: i64 = 7;" }],
    expect: ["MAX_ATTEMPTS is 7 but bus-policies.json dispatch.max_attempts is 5", "must agree"],
  },
  {
    id: "dec058-c",
    what: "bus-policies.json: the first delay already exceeds the cap, so the multiplier does nothing",
    check: GATE,
    edits: [{ file: BUS_POLICIES, find: '"cap_seconds":300', replace: '"cap_seconds":1' }],
    expect: ["backoff.base_seconds (2) exceeds cap_seconds (1)"],
  },

  // --- DEC-056: the wire SHAPE. Five of these must COMPILE, so that a non-zero exit is the conformance test
  // failing rather than the crate failing to build; a mutation that breaks the build proves nothing about the
  // check, because the check never runs.
  {
    id: "dec056-a",
    what: "main.rs: ProjectView is serialized as camelCase again (the original DEC-053 defect)",
    check: RUST,
    edits: [
      {
        file: MAIN,
        find: '#[serde(rename_all = "snake_case")]\nstruct ProjectView {',
        replace: '#[serde(rename_all = "camelCase")]\nstruct ProjectView {',
      },
    ],
    expect: ["create_projectResponse does not conform", 'is missing required key "project_id"'],
  },
  {
    id: "dec056-b",
    what: "main.rs: one response field is renamed on the wire but not in the contract",
    check: RUST,
    edits: [
      {
        file: MAIN,
        find: "struct RecoveryView {\n    clean: bool,\n    integrity_ok: bool,",
        replace: 'struct RecoveryView {\n    clean: bool,\n    #[serde(rename = "integrity")]\n    integrity_ok: bool,',
      },
    ],
    expect: ["get_recovery_statusResponse does not conform", 'is missing required key "integrity_ok"'],
  },
  {
    id: "dec056-c",
    what: "main.rs: a field the contract requires is omitted from the wire",
    check: RUST,
    edits: [
      {
        file: MAIN,
        find: "    brief_body: Option<String>,\n    created_at: String,\n}",
        replace: "    brief_body: Option<String>,\n    #[serde(skip_serializing)]\n    created_at: String,\n}",
      },
    ],
    expect: ['is missing required key "created_at"'],
  },
  {
    id: "dec056-d",
    what: "main.rs: a registered handler is dropped from COVERED_OPERATIONS",
    check: RUST,
    edits: [
      {
        file: MAIN,
        find: '        "create_project",\n        "list_projects",',
        replace: '        "create_project",',
      },
    ],
    expect: ["is registered in generate_handler![...] but no shape test covers it"],
  },
  {
    id: "dec056-e",
    what: "main.rs: COVERED_OPERATIONS names an operation nothing registers",
    check: RUST,
    edits: [
      {
        file: MAIN,
        find: 'const COVERED_OPERATIONS: &[&str] = &[\n        "create_project",',
        replace:
          'const COVERED_OPERATIONS: &[&str] = &[\n        "ghost_operation",\n        "create_project",',
      },
    ],
    expect: ["which generate_handler![...] does not register"],
  },
  {
    id: "dec075-a",
    what: "main.rs: the shell's From<BusError> mapping stops delegating to code(), so it must be producing codes the gate never reads",
    check: GATE,
    edits: [
      {
        file: MAIN,
        find: '            code: error.code(),',
        replace: '            code: "STORAGE_FAILURE",',
      },
    ],
    expect: ["no longer calls code()"],
  },
  {
    id: "dec075-b",
    what: "main.rs: a handler stops being registered, so the bridge loses an operation and its shape test no longer guards anything",
    check: GATE,
    edits: [
      {
        file: MAIN,
        find: '            get_event_cursor',
        replace: '',
      },
    ],
    expect: ["defines #[tauri::command] fn get_event_cursor but never registers it"],
  },
  {
    id: "dec075-c",
    what: "recovery_service.rs: the material-action refusal is removed, so replay becomes a route around authorization",
    check: CORE,
    edits: [
      {
        file: "crates/core/src/recovery_service.rs",
        find: "MATERIAL_ACTION_MESSAGE_TYPES.contains(&facts.message_type.as_str())",
        replace: "false",
      },
    ],
    expect: ["expected an authorization refusal"],
  },
  {
    id: "dec075-d",
    what: "recovery_service.rs: the response claims a context refresh that did not happen",
    check: CORE,
    edits: [
      {
        file: "crates/core/src/recovery_service.rs",
        find: "context_refreshed: false,",
        replace: "context_refreshed: true,",
      },
    ],
    expect: ["context_refreshed"],
  },
  {
    id: "dec075-e",
    what: "main.rs: the authorization refusal emits a code the registry does not declare",
    check: GATE,
    edits: [
      {
        file: MAIN,
        find: 'code: "AUTHORIZATION_NOT_IMPLEMENTED",',
        replace: 'code: "AUTHORIZATION_MISSING",',
      },
    ],
    expect: ["AUTHORIZATION_MISSING"],
  },
  {
    id: "dec075-f",
    what: "bridge.ts: a wrapper sends a camelCase argument name, so the wire spelling drifts from the contract (DEC-054)",
    check: UI,
    edits: [
      {
        file: BRIDGE_TS,
        find: "      consumer_id: consumerId,",
        replace: "      consumerId: consumerId,",
      },
    ],
    expect: ["project_id"],
  },
  {
    id: "dec056-f",
    what: "main.rs: a command stops declaring rename_all, so Tauri's camelCase default decides the wire names",
    check: GATE,
    edits: [
      {
        file: MAIN,
        find: '#[tauri::command(rename_all = "snake_case")]\nfn create_project(',
        replace: "#[tauri::command]\nfn create_project(",
      },
    ],
    expect: [
      'accepts the argument "localPath", which its declared request_fields do not contain',
      'the Rust argument is "local_path"; rename_all decides the wire name',
    ],
  },
  {
    id: "dec056-g",
    what: "main.rs: a handler accepts an argument the contract does not declare",
    check: GATE,
    edits: [
      {
        file: MAIN,
        find: "    local_path: String,\n    initial_brief: String,",
        replace: "    local_path: String,\n    initial_brief: String,\n    display_name: String,",
      },
    ],
    expect: ['accepts the argument "display_name", which its declared request_fields do not contain'],
  },
  {
    id: "dec056-h",
    what: "main.rs: a handler stops accepting a required declared request field",
    check: GATE,
    edits: [
      { file: MAIN, find: "    local_path: String,\n    initial_brief: String,", replace: "    local_path: String," },
    ],
    expect: ['declares the required request field "initial_brief", which the handler does not accept'],
  },

  // --- Controls. These must stay GREEN. A check that fires on prose, or on a contract description, is a check
  // that gets turned off rather than fixed, so the negative direction is tested as deliberately as the positive.
  {
    id: "control-commented-out",
    what: "a COMMENTED-OUT call to an undeclared operation does not fail the gate",
    check: GATE,
    control: true,
    edits: [
      { file: BRIDGE_TS, append: '\n// transport("ghost_in_a_comment", {});\n' },
      { file: APP_TSX, append: '\n// invoke("ghost_in_a_comment", {});\n' },
    ],
    expect: [],
  },
  {
    id: "control-description-only",
    what: "editing only a contract DESCRIPTION does not fail the conformance test",
    check: RUST,
    control: true,
    edits: [
      {
        file: PAYLOAD_TYPES,
        find: "A machine-readable finding kind, so the Control Room does not classify findings by reading their prose.",
        replace: "A machine-readable finding kind. (Prose changed by a mutation control; the shape is unchanged.)",
      },
    ],
    expect: [],
  },
  {
    id: "dec068-a",
    what: "the shipped MAX_PENDING disagrees with bus-policies.json",
    check: GATE,
    edits: [
      {
        file: "crates/bus/src/policy.rs",
        find: "pub const MAX_PENDING: i64 = 1024;",
        replace: "pub const MAX_PENDING: i64 = 64;",
      },
    ],
    expect: ["MAX_PENDING is 64 but bus-policies.json dispatch.max_pending is 1024"],
  },

  // --- DEC-105: hosted CI is permanently banned, so the ban has to be able to fail.
  {
    id: "dec105-a",
    what: ".github/workflows: a GitHub Actions workflow is added back to the repository",
    check: GATE,
    edits: [
      {
        create: true,
        file: ".github/workflows/probe.yml",
        content:
          "name: probe\n" +
          "on:\n" +
          "  push:\n" +
          "jobs:\n" +
          "  noop:\n" +
          "    runs-on: windows-latest\n" +
          "    steps:\n" +
          "      - run: echo not-permitted\n",
      },
    ],
    expect: ["hosted CI artifact present: .github/workflows/probe.yml"],
  },

  // --- DEC-105: the ban is scoped to workflows/ only, and this control holds that scope in place. An issue
  // template is inert metadata GitHub displays and never executes, so it must NOT fail the gate. If the check
  // ever widens back to the whole .github/ tree, this control turns red instead of the widening passing
  // unnoticed - which is the failure mode that produced the original problem.
  {
    id: "dec105-b",
    what: ".github/ISSUE_TEMPLATE: a non-workflow .github/ file does not fail the gate",
    check: GATE,
    control: true,
    edits: [
      {
        create: true,
        file: ".github/ISSUE_TEMPLATE/bug.md",
        content: "---\nname: Bug report\nabout: Something is wrong\n---\n\nDescribe it.\n",
      },
    ],
    expect: [],
  },
  // --- The traceability check. It enforces four properties, and a check that enforces four things while being
  // proven against one of them is proven against one of them, so each is mutated separately. docs/TRACEABILITY.md
  // had never been read by anything before this check, which is why its rows could claim VALIDATED with no test
  // behind them and cite evidence paths that nothing resolved.
  {
    id: "trace-a",
    what: "TRACEABILITY.md: a state word outside the vocabulary the document declares",
    check: GATE,
    edits: [
      {
        file: TRACEABILITY,
        find: "the code is never mapped to a non-backoff registry code | DECIDED |",
        replace: "the code is never mapped to a non-backoff registry code | SORT_OF |",
      },
    ],
    expect: ['state "SORT_OF" is not in the vocabulary this document declares'],
  },
  {
    id: "trace-b",
    what: "TRACEABILITY.md: an evidence path that does not exist",
    check: GATE,
    edits: [
      {
        file: TRACEABILITY,
        find: "crates/core/src/attachment_service.rs",
        replace: "crates/core/src/attachment_services.rs",
      },
    ],
    expect: ["does not exist in the repository"],
  },
  {
    id: "trace-c",
    what: "TRACEABILITY.md: a cited decision that has no register entry",
    check: GATE,
    edits: [
      {
        file: TRACEABILITY,
        find: "capture to `EvidenceService` | DEC-107 |",
        replace: "capture to `EvidenceService` | DEC-999 |",
      },
    ],
    expect: ["cites DEC-999, which has no entry in docs/DECISION-REGISTER.md"],
  },
  {
    id: "trace-d",
    what: "TRACEABILITY.md: a VALIDATED row whose evidence cell asserts rather than cites",
    check: GATE,
    edits: [
      {
        file: TRACEABILITY,
        find: "| A selected file or folder becomes a durable reference carrying its provenance, and nothing else | DEC-106 | `attach_records_a_reference_with_provenance_and_no_content_identity` in `crates/core/tests/attachment_slice.rs` | VALIDATED |",
        replace:
          "| A selected file or folder becomes a durable reference carrying its provenance, and nothing else | DEC-106 | the behaviour is implemented and obviously correct | VALIDATED |",
      },
    ],
    expect: ["claims VALIDATED and cites no evidence"],
  },
  {
    id: "trace-e",
    what: "DECISION-REGISTER.md: two index rows joined by a lost newline, so the table is no longer a table",
    check: GATE,
    edits: [
      {
        file: DECISION_REGISTER,
        find: "| ADDITIVE |\n| DEC-075 | Wiring replay",
        replace: "| ADDITIVE || DEC-075 | Wiring replay",
      },
    ],
    expect: ["carries 2 index-table rows"],
  },
  {
    id: "trace-f",
    what: "DECISION-REGISTER.md: a DEC heading appended to the paragraph before it, so the heading is not a heading",
    check: GATE,
    edits: [
      {
        file: DECISION_REGISTER,
        find: "guard.\n### DEC-097 — Lease fence rotates only on ownership change",
        replace: "guard.### DEC-097 — Lease fence rotates only on ownership change",
      },
    ],
    expect: ["carries a DEC heading after other text"],
  },
  // --- The comment scanner. Every frontend and Rust scan in the gate reads code with comments removed, so a
  // scanner that loses track of where the code is makes all of those scans wrong while still returning a
  // plausible string. It was wrong in two ways, and both were silent until the guard existed: an apostrophe in
  // JSX text opened a string that never closed, and a Rust raw string was read as a plain one. The guard is
  // proven here so that the next construct it cannot read fails loudly instead of quietly.
  {
    id: "scanner-a",
    what: "a frontend file with an unterminated string, so the comment scanner cannot tell code from text",
    check: GATE,
    edits: [{ file: APP_TSX, append: '\nconst probe = "unterminated\n' }],
    expect: ["the comment scanner lost track of the code"],
  },
  {
    id: "control-scanner-apostrophe",
    what: "an apostrophe in JSX text does not desynchronize the scanner, so a commented-out invoke stays invisible",
    check: GATE,
    control: true,
    edits: [
      {
        file: APP_TSX,
        append: "\nconst probe = <p>the project's recorded references</p>;\n// invoke(\"ghost_in_a_comment\", {});\n",
      },
    ],
    expect: [],
  },
  // --- The historical integrity audit. Two properties matter and they pull in opposite directions: it has to see
  // text that a later edit destroyed, and it must not see two different lines that merely share a prefix. The
  // first mutation proves the first. The control proves the second, and it is the case that actually produced a
  // false positive: `CouncilRound` and `CouncilRoundRole` are two separate types in DATA-MODEL.md, and the first
  // is a prefix of the second. A prefix match is not corruption; only a prefix match whose continuation is gone
  // is, which is why the audit requires the longer line to be absent from the current text.
  {
    id: "integrity-a",
    what: "a document line truncated mid-word, so text that every revision contains is missing from the tree",
    check: INTEGRITY,
    edits: [
      {
        file: TRACEABILITY,
        find: "It is a durable local reference with\nprovenance (DEC-106)",
        replace: "It is a durable local referen\nprovenance (DEC-106)",
      },
    ],
    expect: ["is a word-severed prefix of an earlier revision"],
  },
  {
    id: "control-integrity-prefix",
    what: "a line that is a mid-word prefix of a longer line still present in full, so a prefix match alone is not corruption",
    check: INTEGRITY,
    control: true,
    edits: [
      {
        file: DATA_MODEL,
        append:
          "\nCouncilSession and CouncilRound are persisted in SQLite with participants, positions, questions and\n",
      },
    ],
    expect: [],
  },
  {
    id: "control-trace-prose",
    what: "TRACEABILITY.md: prose is not scanned, so a decision or a path named outside a table is ignored",
    check: GATE,
    control: true,
    edits: [
      {
        file: TRACEABILITY,
        find: "An attachment is context and evidence, never project truth (DEC-049).",
        replace:
          "An attachment is context and evidence, never project truth (DEC-049). DEC-999 and `docs/NOT-A-REAL-FILE.md` are named in prose here, and prose is not a table.",
      },
    ],
    expect: [],
  },
  // The owner of an operation is declared twice: in `workspace.manifest.json`, which the bridge identifier
  // checks read and `payloads.schema.json` names as the resolver, and in `payloads.json`, which
  // `tools/codegen/generate-bridge.mjs` reads to emit the owner map the frontend routes calls by. Nothing
  // compared the two until this check existed, and they had already drifted: `create_trace_link` was
  // DiagnosticsService in one file and RequirementService in the other, so the generated UI map named a
  // different authority than the contract did. These three prove the comparison fires on a missing owner and on
  // a disagreeing one, and that it reads the owner field rather than whatever prose happens to sit beside it.
  {
    id: "contrib-a",
    what: "workspace.manifest.json: a declared command with no owner",
    check: GATE,
    edits: [
      {
        file: MANIFEST,
        find: '      "attach_project_context_attachment": "AttachmentService",\n      "record_user_contribution": "ProjectService"\n    },',
        replace: '      "attach_project_context_attachment": "AttachmentService"\n    },',
      },
    ],
    expect: ["No command owner: record_user_contribution"],
  },
  {
    id: "contrib-b",
    what: "payloads.json: an operation owner that disagrees with workspace.manifest.json",
    check: GATE,
    edits: [
      {
        file: PAYLOADS,
        find: '    "record_user_contribution": {\n      "owner": "ProjectService",',
        replace: '    "record_user_contribution": {\n      "owner": "AttachmentService",',
      },
    ],
    expect: [
      'is owned by "ProjectService" in workspace.manifest.json but "AttachmentService"',
    ],
  },
  {
    id: "control-contrib-prose",
    what: "payloads.json: naming a service in an operation's prose is not an ownership declaration",
    check: GATE,
    control: true,
    edits: [
      {
        file: PAYLOADS,
        find: '"atomicity": "one durable write: the contribution row is inserted and read back.',
        replace:
          '"atomicity": "AttachmentService is named here in prose, and the owner field is what the check reads. one durable write: the contribution row is inserted and read back.',
      },
    ],
    expect: [],
  },

  // --- DEC-109: the failure class vocabulary, the class -> recovery action mapping, and the repair budgets.
  // Every one of these reintroduces a divergence that was actually possible before the checks existed: a failure
  // packet carrying a class no registry entry uses, a recovery action naming an operation the contract does not
  // declare, a class with no stated handling, and an unclassifiable failure recorded as a success.
  {
    id: "dec109-a",
    what: "failure-class-policies.json: a class maps an operation the service registry does not declare",
    check: GATE,
    edits: [
      {
        file: FAILURE_CLASS_POLICIES,
        find: '"TIMEOUT":{"action":"TaskService.retry_task"',
        replace: '"TIMEOUT":{"action":"TaskService.does_not_exist"',
      },
    ],
    expect: [
      'TIMEOUT maps recovery action "TaskService.does_not_exist", which schemas/service-contracts-v1/registry.json does not declare',
    ],
  },
  {
    id: "dec109-b",
    what: "failure-class-policies.json: a class the MCF category enum declares is left with no handling",
    check: GATE,
    edits: [{ file: FAILURE_CLASS_POLICIES, find: '"INTERNAL":{', replace: '"INTERNAL_UNMAPPED":{' }],
    expect: ["maps no recovery action for failure class(es): INTERNAL"],
  },
  {
    id: "dec109-c",
    what: "failure-class-policies.json: a class the MCF category enum does not declare is invented",
    check: GATE,
    edits: [
      {
        file: FAILURE_CLASS_POLICIES,
        find: '"classes":{',
        replace:
          '"classes":{"TEXT_CONFLICT":{"action":"RecoveryService.start_recovery","rationale":"invented"},',
      },
    ],
    expect: ["maps class(es) that are not in the MCF category enum: TEXT_CONFLICT"],
  },
  {
    id: "dec109-d",
    what: "failure-class-policies.json: an unclassifiable failure is recorded as a success (DEC-083)",
    check: GATE,
    edits: [{ file: FAILURE_CLASS_POLICIES, find: '"is_success":false', replace: '"is_success":true' }],
    expect: ["the fallback declares is_success other than false; an unclassified failure is never a success (DEC-083)"],
  },
  {
    id: "dec109-e",
    what: "failure-class-policies.json: the fallback claims to be a real failure class",
    check: GATE,
    edits: [{ file: FAILURE_CLASS_POLICIES, find: '"failure_class":"UNKNOWN"', replace: '"failure_class":"INTERNAL"' }],
    expect: ['the fallback class is "INTERNAL"', "must be recorded as UNKNOWN (DEC-083)"],
  },
  {
    id: "dec109-f",
    what: "failure.schema.json: the category enum drops a class the MCF enum declares",
    check: GATE,
    edits: [{ file: FAILURE_SCHEMA, find: '"LEASE","WORKSPACE"', replace: '"WORKSPACE"' }],
    expect: ["is missing failure class(es) the MCF category enum declares: LEASE"],
  },
  {
    id: "dec109-g",
    what: "failure.schema.json: category goes back to free text, so any class is accepted",
    check: GATE,
    edits: [
      {
        file: FAILURE_SCHEMA,
        find: '"category":{"type":"string","enum":["SCHEMA","IDENTITY","AUTHORIZATION","POLICY","CAPABILITY","CONTEXT","LEASE","WORKSPACE","SEQUENCE","IDEMPOTENCY","TIMEOUT","CANCELLATION","PROCESS","ADAPTER","AUTHENTICATION","VERSION","DEAD_LETTER","INTERNAL"]}',
        replace: '"category":{"type":"string"}',
      },
    ],
    expect: ["declares no `category` enum, so a failure packet can carry a class no registry entry uses"],
  },
  {
    id: "dec109-h",
    what: "recovery.schema.json: the action enum admits an operation no failure class maps to",
    check: GATE,
    edits: [
      {
        file: RECOVERY_SCHEMA,
        find: '"RecoveryService.resolve_recovery"]}',
        replace: '"RecoveryService.resolve_recovery","LifecycleService.abandon_project"]}',
      },
    ],
    expect: ["declares action(s) no failure class maps to: LifecycleService.abandon_project"],
  },
  {
    id: "dec109-i",
    what: "recovery.schema.json: an action whose result is unknown has no outcome to record (DEC-083)",
    check: GATE,
    edits: [
      {
        file: RECOVERY_SCHEMA,
        find: '"outcome":{"enum":["SUCCEEDED","FAILED","BLOCKED","UNKNOWN"]}',
        replace: '"outcome":{"enum":["SUCCEEDED","FAILED","BLOCKED"]}',
      },
    ],
    expect: ["the recovery action outcome enum has no UNKNOWN member"],
  },
  {
    id: "dec109-j",
    what: "repair-policies.json: the budgets are unsatisfiable, tolerating more regressions than attempts",
    check: GATE,
    edits: [
      { file: REPAIR_POLICIES, find: '"max_consecutive_regressions":2', replace: '"max_consecutive_regressions":99' },
    ],
    expect: ["tolerates more consecutive regressions than total attempts, which no run can satisfy"],
  },
  {
    id: "dec109-k",
    what: "repair-policies.json: the rule against deleting a test as a repair is turned off",
    check: GATE,
    edits: [
      {
        file: REPAIR_POLICIES,
        find: '"forbid_test_deletion_as_repair":true',
        replace: '"forbid_test_deletion_as_repair":false',
      },
    ],
    expect: ["does not forbid deleting a test as a repair, which AGENTS.md section 20 forbids unconditionally"],
  },
  {
    id: "dec109-l",
    what: "repair-policies.json: a repair budget becomes zero, so no repair may ever be attempted",
    check: GATE,
    edits: [
      { file: REPAIR_POLICIES, find: '"max_attempts_per_fingerprint":3', replace: '"max_attempts_per_fingerprint":0' },
    ],
    expect: ["declares max_attempts_per_fingerprint as 0; a repair budget must be a positive whole number"],
  },
  {
    id: "control-dec109-prose",
    what: "failure-class-policies.json: the rule text is reworded, which is prose and not a contract",
    check: GATE,
    edits: [
      {
        file: FAILURE_CLASS_POLICIES,
        find: '"rule":"This file owns one thing',
        replace: '"rule":"This file owns exactly one thing',
      },
    ],
    expect: [],
  },
];

// -----------------------------------------------------------------------------------------------------------
// Running the checks.
// -----------------------------------------------------------------------------------------------------------

const run = (cmd, args) => {
  const result = spawnSync(cmd, args, { cwd: root, encoding: "utf8", maxBuffer: 128 * 1024 * 1024 });
  if (result.error) return { code: null, output: String(result.error) };
  return { code: result.status, output: `${result.stdout ?? ""}${result.stderr ?? ""}` };
};

/**
 * The frontend test files, read from the script that runs them.
 *
 * Returns them as repo-relative paths, because the harness runs every command from the repository root. Throws
 * rather than returning an empty list: a check that silently runs no tests would report success, which is the
 * one failure mode a check must not have.
 *
 * The script may name files or a glob, and a glob may be quoted so that a shell does not expand it. Both are
 * resolved here instead of being handed to node, because handing a quoted glob straight through is how this
 * check silently ran zero tests: `spawnSync` does not expand it, node matched nothing, and a run with no tests
 * exits 0, so every frontend mutation reported "passed". The empty-list guard below did not catch it, because
 * the list was not empty - it held one unusable entry.
 */
function uiTestFiles() {
  const script = JSON.parse(
    fs.readFileSync(`${root}/apps/desktop/package.json`, "utf8"),
  ).scripts.test;
  const prefix = "node --experimental-strip-types --test ";
  if (!script.startsWith(prefix)) {
    throw new Error(`could not read the frontend test file list from apps/desktop/package.json: ${script}`);
  }
  const patterns = script
    .slice(prefix.length)
    .split(/\s+/)
    .map((token) => token.replace(/^["']|["']$/g, ""))
    .filter(Boolean);
  const files = [];
  for (const pattern of patterns) {
    if (!/[*?]/.test(pattern)) {
      files.push(`apps/desktop/${pattern}`);
      continue;
    }
    for (const match of fs.globSync(pattern, { cwd: `${root}/apps/desktop` })) {
      files.push(`apps/desktop/${match.replace(/\\/g, "/")}`);
    }
  }
  const missing = files.filter((file) => !fs.existsSync(`${root}/${file}`));
  if (files.length === 0 || missing.length) {
    throw new Error(
      `the frontend test script in apps/desktop/package.json resolved to no usable test file` +
        `${missing.length ? ` (missing: ${missing.join(", ")})` : ""}: ${script}`,
    );
  }
  return files;
}

const CHECKS = {
  [GATE]: {
    name: "the contract gate",
    command: "npm run verify:contracts",
    run: () => run("node", ["tools/contracts/verify.mjs"]),
  },
  [CORE]: {
    name: "the core crate's tests",
    command: "cargo test -p mayasaba-core",
    run: () => run("cargo", ["test", "-p", "mayasaba-core"]),
  },
  [UI]: {
    name: "the frontend bridge tests",
    command: "npm --prefix apps/desktop test",
    run: () => run("node", ["--experimental-strip-types", "--test", ...uiTestFiles()]),
  },
  [INTEGRITY]: {
    name: "the historical integrity audit",
    command: "npm run verify:integrity",
    run: () => run("node", ["tools/verify/integrity.mjs"]),
  },
  [RUST]: {
    name: "the wire-shape conformance test",
    command: "cargo test -p mayasaba-desktop",
    run: () => run("cargo", ["test", "-p", "mayasaba-desktop"]),
  },
};

// -----------------------------------------------------------------------------------------------------------
// Guards: a clean tree is what makes `git checkout -- <file>` a complete restore.
//
// Untracked files are permitted, because `git checkout -- <file>` never touches them: it restores from the
// index, so it can only discard changes to tracked files. That distinction matters in practice — this file
// itself is usually untracked the first time it is run — and it does not weaken the guard, because the thing
// the guard exists to prevent is discarding someone's edits to a tracked file.
// -----------------------------------------------------------------------------------------------------------

const git = (args) => run("git", args);

const statusBefore = git(["status", "--porcelain"]);
if (statusBefore.code !== 0) {
  console.error(
    "Cannot run mutation tests: this is not a usable git work tree.\n" +
      "The harness restores mutated files with `git checkout -- <file>`, so it needs git to be able to undo it.\n" +
      statusBefore.output.trim()
  );
  process.exit(2);
}
const atRisk = statusBefore.output
  .split("\n")
  .filter((line) => line.trim() && !line.startsWith("??"));
if (atRisk.length) {
  console.error(
    "Cannot run mutation tests: tracked files have uncommitted changes.\n\n" +
      atRisk.join("\n") +
      "\n\nThe harness mutates tracked files and restores them with `git checkout -- <file>`, which restores from\n" +
      "the index and would therefore discard the changes listed above along with its own. Commit or stash them\n" +
      "first. (Untracked files are fine and are not listed: `git checkout` cannot touch them.)"
  );
  process.exit(2);
}

const headBefore = git(["rev-parse", "HEAD"]).output.trim();
const touched = new Set();
const created = new Set();

const restore = () => {
  const files = [...touched];
  if (files.length) git(["checkout", "--", ...files]);
  touched.clear();
  // A file this harness created is untracked, so `git checkout -- <file>` cannot remove it, and the
  // untracked-tolerant verification below would not report it either. Created files are therefore deleted
  // here and their now-empty directories pruned. This matters more than tidiness: the mutation that proves
  // hosted CI is banned creates a .github/workflows/ file, and leaving that behind would be a real instance
  // of the violation the mutation exists to demonstrate is caught.
  for (const file of created) {
    const full = path.join(root, file);
    fs.rmSync(full, { force: true });
    let dir = path.dirname(full);
    while (
      dir !== root &&
      dir.startsWith(root + path.sep) &&
      fs.existsSync(dir) &&
      fs.readdirSync(dir).length === 0
    ) {
      fs.rmdirSync(dir);
      dir = path.dirname(dir);
    }
  }
  created.clear();
  return git(["status", "--porcelain"])
    .output.split("\n")
    .filter((line) => line.trim() && !line.startsWith("??"))
    .join("\n")
    .trim();
};

const reportUnrestorable = (detail) => {
  console.error(
    "\n" +
      "=".repeat(100) +
      "\nTHE WORKING TREE COULD NOT BE FULLY RESTORED. The recovery is:\n\n" +
      "    git checkout -- .\n\n" +
      "That discards ALL uncommitted changes to tracked files. It is correct here because the harness refused to\n" +
      "start with a modified tracked file, so everything uncommitted is the harness's own.\n" +
      (detail ? `\n${detail}\n` : "") +
      "=".repeat(100)
  );
};

// Restore on every exit path. An interrupted run that left a mutation in place would be worse than a failed
// run, because the next `git commit` would carry it.
process.on("SIGINT", () => {
  const left = restore();
  console.error("\nInterrupted. Restored every mutated file.");
  if (left) reportUnrestorable(`Still modified:\n${left}`);
  process.exit(130);
});
process.on("uncaughtException", (error) => {
  const left = restore();
  console.error(`\nUnhandled failure: ${error?.stack ?? error}`);
  if (left) reportUnrestorable(`Still modified:\n${left}`);
  process.exit(1);
});

// -----------------------------------------------------------------------------------------------------------
// Applying a mutation.
// -----------------------------------------------------------------------------------------------------------

const applyEdits = (mutation) => {
  const pending = new Map();
  // A mutation's `find` and `replace` are written with LF, because that is the convention of the files
  // themselves. On disk a file may be CRLF: `core.autocrlf=true` smudges everything git checks out that
  // .gitattributes does not pin to LF, and this harness restores a mutated file with `git checkout -- <file>`.
  // So the first multi-line anchor aimed at an unpinned file converted its own target, and every later anchor
  // in that file stopped matching - a harness error that reports the mutation as broken rather than the check.
  // No multi-line anchor had ever been aimed outside a .rs file, which .gitattributes does pin, so this was
  // latent. Content is normalized to LF for matching and written back in the convention it arrived in, so an
  // anchor matches on either checkout and the harness leaves no spurious diff behind.
  const crlf = new Map();
  const read = (file) => {
    if (!pending.has(file)) {
      const raw = fs.readFileSync(path.join(root, file), "utf8");
      const usesCrlf = raw.includes("\r\n");
      crlf.set(file, usesCrlf);
      pending.set(file, usesCrlf ? raw.replace(/\r\n/g, "\n") : raw);
    }
    return pending.get(file);
  };
  for (const edit of mutation.edits) {
    if (edit.create === true) {
      const full = path.join(root, edit.file);
      if (fs.existsSync(full)) {
        throw new Error(
          `${mutation.id}: the create target ${edit.file} already exists. A create mutation must introduce a ` +
            `file that is not there, so this is a harness defect rather than a check result.`
        );
      }
      fs.mkdirSync(path.dirname(full), { recursive: true });
      fs.writeFileSync(full, edit.content ?? "");
      created.add(edit.file);
      continue;
    }
    const content = read(edit.file);
    if (edit.append !== undefined) {
      pending.set(edit.file, content + edit.append);
      continue;
    }
    const occurrences = content.split(edit.find).length - 1;
    if (occurrences === 0) {
      throw new Error(
        `${mutation.id}: the anchor was not found in ${edit.file}, so the mutation was never applied and the ` +
          `check below would have been run against an unmutated file:\n${edit.find}`
      );
    }
    if (occurrences > 1 && !edit.all) {
      throw new Error(
        `${mutation.id}: the anchor matches ${occurrences} times in ${edit.file}. An ambiguous mutation is not a ` +
          `mutation, so this is a harness defect rather than a check result. Make the anchor unique, or set ` +
          `all: true to mean "every occurrence".\n${edit.find}`
      );
    }
    pending.set(
      edit.file,
      edit.all ? content.split(edit.find).join(edit.replace) : content.replace(edit.find, edit.replace)
    );
  }
  for (const [file, content] of pending) {
    fs.writeFileSync(path.join(root, file), crlf.get(file) ? content.replace(/\n/g, "\r\n") : content);
    touched.add(file);
  }
};

// -----------------------------------------------------------------------------------------------------------
// The run.
// -----------------------------------------------------------------------------------------------------------

const filterArg = process.argv.indexOf("--filter");
const filter = filterArg >= 0 ? process.argv[filterArg + 1] : null;
if (filterArg >= 0 && !filter) {
  console.error("--filter needs a value, e.g. --filter dec056");
  process.exit(2);
}
const selected = filter ? MUTATIONS.filter((m) => m.id.includes(filter)) : MUTATIONS;
if (!selected.length) {
  console.error(`No mutation id contains "${filter}".`);
  process.exit(2);
}

console.log("Mutation tests for the contract gate and the wire-shape conformance test");
console.log(`Repository: ${root}`);
console.log(`HEAD: ${headBefore}`);
console.log(`${selected.length} mutation(s) selected${filter ? ` by --filter ${filter}` : ""}\n`);

// The unmutated controls. Without these a mutation's failure could be attributable to the tree rather than to
// the mutation, which is exactly the mistake that makes a mutation suite misleading rather than reassuring.
const checksToControl = [...new Set(selected.map((m) => m.check))];
let controlFailures = 0;
for (const key of checksToControl) {
  const check = CHECKS[key];
  process.stdout.write(`Control: ${check.command} on the unmutated tree ... `);
  const result = check.run();
  if (result.code === 0) {
    console.log("exit 0, as expected");
  } else {
    console.log(`exit ${result.code}, WHICH IS WRONG — the tree is not green before any mutation`);
    console.log(result.output.trimEnd().split("\n").slice(-25).map((l) => `    ${l}`).join("\n"));
    controlFailures += 1;
  }
}
if (controlFailures) {
  console.error(
    `\n${controlFailures} control(s) failed, so a mutation's failure could not be attributed to the mutation. ` +
      `Fix the tree first; the mutation results below would be meaningless.`
  );
  process.exit(1);
}

console.log("");
let passed = 0;
const failures = [];

selected.forEach((mutation, index) => {
  const check = CHECKS[mutation.check];
  const label = `${String(index + 1).padStart(2)}/${selected.length} ${mutation.id}`;
  process.stdout.write(`${label}  ${mutation.what}\n${" ".repeat(label.length)}  ${check.command} ... `);
  let result;
  try {
    applyEdits(mutation);
    result = check.run();
  } catch (error) {
    restore();
    console.log("HARNESS ERROR");
    console.log(`    ${error.message.split("\n").join("\n    ")}`);
    failures.push({ mutation, reason: error.message });
    return;
  }
  const left = restore();

  if (mutation.control) {
    if (result.code === 0) {
      passed += 1;
      console.log("exit 0, as expected for a control");
    } else {
      console.log(`exit ${result.code}, WHICH IS WRONG — a control must stay green`);
      console.log(result.output.trimEnd().split("\n").slice(-15).map((l) => `    ${l}`).join("\n"));
      failures.push({ mutation, reason: `control exited ${result.code}` });
    }
  } else if (result.code === null || result.code === 0) {
    console.log(`exit ${result.code}, WHICH IS WRONG — the mutation was not detected`);
    console.log("    (a check that passes on a mutated tree does not check what it claims to)");
    failures.push({ mutation, reason: `not detected (exit ${result.code})` });
  } else {
    const missing = mutation.expect.filter((needle) => !result.output.includes(needle));
    if (missing.length) {
      console.log(`exit ${result.code}, but for the wrong reason`);
      for (const needle of missing) console.log(`    the output never says: ${needle}`);
      console.log(result.output.trimEnd().split("\n").slice(-15).map((l) => `    ${l}`).join("\n"));
      failures.push({ mutation, reason: `wrong reason (exit ${result.code})` });
    } else {
      passed += 1;
      console.log(`exit ${result.code}, naming ${mutation.expect.length || "the"} expected disagreement(s)`);
    }
  }

  if (left) {
    console.log(`    WARNING: the tree is still modified after restore:\n${left}`);
    reportUnrestorable();
    process.exit(1);
  }
});

// -----------------------------------------------------------------------------------------------------------
// The verdict.
// -----------------------------------------------------------------------------------------------------------

const headAfter = git(["rev-parse", "HEAD"]).output.trim();
// Only tracked files are the harness's business; untracked ones were there before it started and it cannot
// touch them, so they must not be reported as a failed restore.
const statusAfter = git(["status", "--porcelain"])
  .output.split("\n")
  .filter((line) => line.trim() && !line.startsWith("??"))
  .join("\n")
  .trim();

console.log("");
if (headAfter !== headBefore) {
  console.error(`HEAD moved during the run (${headBefore} -> ${headAfter}). Nothing here commits; investigate.`);
  process.exit(1);
}
if (statusAfter) {
  console.error(`The working tree is not clean after the run:\n${statusAfter}`);
  reportUnrestorable();
  process.exit(1);
}

if (failures.length) {
  console.error(`${passed} of ${selected.length} behaved as expected. ${failures.length} did not:`);
  for (const { mutation, reason } of failures) console.error(`  - ${mutation.id}: ${reason}`);
  process.exit(1);
}

console.log(
  `${passed} of ${selected.length} behaved as expected. The working tree is clean and HEAD is unchanged ` +
    `(${headAfter.slice(0, 7)}).`
);
