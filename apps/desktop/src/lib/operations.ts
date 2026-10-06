// What the Control Room can actually call, and what it cannot.
//
// The bridge contract declares 63 operations. Twelve of them have a handler registered in
// `generate_handler![...]`, so the other 51 are declared and unimplemented - the contract deliberately leads
// the implementation (verify.mjs states this at the bridge check). A Control Room that rendered a section for
// every declared operation would be showing 51 surfaces that cannot answer, and the failure mode is not a
// crash: it is a panel that looks like it is waiting for data when there is nothing to wait for.
//
// So the surface is partitioned here, once, and every section renders from this partition. An operation with
// no handler renders as declared-but-unimplemented, naming the operation and its owning service, rather than
// as an empty panel or a fabricated value.
//
// `REGISTERED_HANDLERS` is the one hand-maintained list in this file, and it is not trusted: a test reads
// `src-tauri/src/main.rs` and asserts this list equals the `generate_handler![...]` list exactly, in both
// directions. That is the same discipline the bridge gate applies to the Rust side, applied to the copy the
// UI reads, so the UI cannot claim a handler exists that the shell does not register.

// The `.ts` extension is required rather than stylistic: this is a value import, and the desktop test runner
// executes these modules directly under Node, which resolves ESM specifiers literally.
import { COMMANDS, COMMAND_OWNERS, QUERIES, QUERY_OWNERS } from "../generated/bridge.ts";

export type OperationKind = "command" | "query";

export interface Operation {
  name: string;
  kind: OperationKind;
  /** The owning application service, from payloads.json. The UI routes to it; it never acts for it. */
  owner: string;
  /** True when the Tauri shell registers a handler for this operation. */
  implemented: boolean;
}

/**
 * The handlers `apps/desktop/src-tauri/src/main.rs` registers.
 *
 * Cross-checked against that file by `operations.test.ts`, so this cannot drift from the shell it describes.
 */
export const REGISTERED_HANDLERS: readonly string[] = [
  "create_project",
  "list_projects",
  "get_recovery_status",
  "get_agent_status",
  "validate_workspace",
  "replay_dead_letter",
  "get_communication_health",
  "get_event_cursor",
  "attach_project_context_attachment",
  "list_project_context_attachments",
  "resolve_project_context_attachment",
  "record_user_contribution",
];

const toOperation = (kind: OperationKind) => (name: string): Operation => ({
  name,
  kind,
  owner: (kind === "command" ? COMMAND_OWNERS : QUERY_OWNERS)[name as never] as string,
  implemented: REGISTERED_HANDLERS.includes(name),
});

/** Every declared operation, commands first, in contract order. */
export const OPERATIONS: readonly Operation[] = [
  ...COMMANDS.map(toOperation("command")),
  ...QUERIES.map(toOperation("query")),
];

const BY_NAME = new Map(OPERATIONS.map((operation) => [operation.name, operation]));

export function operation(name: string): Operation {
  const found = BY_NAME.get(name);
  if (!found) {
    // A name that is not in the contract is a programming error in this file, not a runtime condition: the
    // generated surface is the authority and every name below is checked against it by the test.
    throw new Error(`${name} is not a declared bridge operation`);
  }
  return found;
}

export function operations(names: readonly string[]): Operation[] {
  return names.map(operation);
}

/** The declared operations with no registered handler. */
export const UNIMPLEMENTED: readonly Operation[] = OPERATIONS.filter((o) => !o.implemented);

/**
 * The 15 primary-navigation sections, in the order CONTROL-ROOM-DESIGN.md lists them.
 *
 * Each section names the operations it is built from. The partition is exact and total - every declared
 * operation belongs to exactly one section or to the shell - and `operations.test.ts` asserts that, so a
 * newly declared operation cannot be silently absent from the navigation.
 */
export interface Section {
  id: string;
  label: string;
  /** One line stating what the section is for, taken from CONTROL-ROOM-DESIGN.md. */
  purpose: string;
  operations: readonly string[];
}

export const SECTIONS: readonly Section[] = [
  {
    id: "chat",
    label: "Chat",
    purpose: "Free text recorded as a UserContribution, with the outcome the owning service produced.",
    operations: ["record_user_contribution"],
  },
  {
    id: "council",
    label: "Council",
    purpose: "Round state, participation, proposals, critiques, disagreements and the round's termination.",
    operations: ["answer_user_question", "get_council_round"],
  },
  {
    id: "requirements",
    label: "Requirements",
    purpose: "Requirements and the trace links that connect them to what satisfies them.",
    operations: ["upsert_requirement", "create_trace_link", "list_requirements"],
  },
  {
    id: "architecture",
    label: "Architecture",
    purpose: "The architecture record and its revisions.",
    operations: ["upsert_architecture", "get_architecture"],
  },
  {
    id: "decisions",
    label: "Decisions",
    purpose: "Decision records, their locks, and reopening a decision that was locked.",
    operations: ["upsert_decision", "reopen_decision", "list_decisions"],
  },
  {
    id: "tasks",
    label: "Tasks",
    purpose: "The task graph, leases, retries and reassignment.",
    operations: ["retry_task", "reassign_task", "get_task_graph"],
  },
  {
    id: "agents",
    label: "Agents",
    purpose: "Runtime identity, session, capabilities, health, active task, lease and recent evidence.",
    operations: ["launch_agent", "stop_agent", "pause_agent", "resume_agent", "get_agent_status"],
  },
  {
    id: "files",
    label: "Files",
    purpose: "The authorized workspace scope, its checkpoints, and the context referenced in place.",
    operations: [
      "attach_project_context_attachment",
      "rollback_workspace",
      "get_workspace_status",
      "validate_workspace",
      "list_project_context_attachments",
      "resolve_project_context_attachment",
    ],
  },
  {
    id: "build",
    label: "Build",
    purpose: "Build health for software tasks.",
    operations: ["get_build_status"],
  },
  {
    id: "run",
    label: "Run/Preview",
    purpose: "Process status and runtime verification, reported separately from launch status.",
    operations: ["request_preview", "start_simulation", "stop_simulation", "get_simulation_status"],
  },
  {
    id: "tests",
    label: "Tests",
    purpose: "Test runs and the validation gate's verdict.",
    operations: ["get_test_runs", "get_validation_status"],
  },
  {
    id: "repairs",
    label: "Repairs",
    purpose: "Failures, their repair state, and retrying a repair.",
    operations: ["retry_repair", "get_failures", "get_repairs"],
  },
  {
    id: "logs",
    label: "Logs",
    purpose: "Diagnostics, communication health, the durable event cursor and dead-letter replay.",
    operations: [
      "request_event_resync",
      "replay_event",
      "replay_dead_letter",
      "get_logs",
      "get_communication_health",
      "get_event_cursor",
      "get_doctor_report",
    ],
  },
  {
    id: "evidence",
    label: "Evidence",
    purpose: "Claims linked to evidence, and the reviews recorded against them.",
    operations: ["get_evidence", "get_reviews"],
  },
  {
    id: "settings",
    label: "Settings",
    purpose: "Configuration, its validation, and the preflight doctor report.",
    operations: ["update_configuration", "validate_configuration", "get_configuration"],
  },
];

/**
 * Operations the persistent shell owns rather than a section.
 *
 * The header's lifecycle controls, the project scope, the recovery banner and the context rail are always
 * present, so their operations belong to the shell. They are listed rather than left out so the partition
 * stays total and a new operation has to be placed deliberately.
 */
export const SHELL_OPERATIONS: readonly string[] = [
  "create_project",
  "open_project",
  "pause_project",
  "resume_project",
  "stop_project",
  "abandon_project",
  "approve_action",
  "start_recovery",
  "resolve_recovery",
  "request_sync",
  "get_project",
  "list_projects",
  "get_project_status",
  "get_context_status",
  "get_recovery_status",
  "get_action_admissibility",
];

/** Every operation named by a section or by the shell. Used by the test to prove the partition is total. */
export const PLACED_OPERATIONS: readonly string[] = [
  ...SECTIONS.flatMap((section) => section.operations),
  ...SHELL_OPERATIONS,
];

export function sectionById(id: string): Section {
  const found = SECTIONS.find((section) => section.id === id);
  if (!found) throw new Error(`${id} is not a Control Room section`);
  return found;
}

/** The operations of a section that the shell can actually call. */
export function implementedIn(section: Section): Operation[] {
  return operations(section.operations).filter((o) => o.implemented);
}

/** The operations of a section that are declared but have no handler. */
export function unimplementedIn(section: Section): Operation[] {
  return operations(section.operations).filter((o) => !o.implemented);
}
