// Tests for the intake state machine and the authority boundary.
//
// Run with: npm test
//
// The load-bearing test is in `authority boundary`: after success the UI must display what the service
// committed, never the submitted draft. The draft has no name field at all, because the display name is
// derived by Rust from the workspace folder (DEC-050) - so the authority property is proven on the brief
// body, which the service does normalize.

import assert from "node:assert/strict";
import { test, describe } from "node:test";

import {
  EMPTY_DRAFT,
  draftOf,
  hasDraft,
  initialState,
  intakeReducer,
  isSubmitting,
  type CommandError,
  type Draft,
  type IntakeState,
  type ProjectView,
} from "./state.ts";
import { createProject, resetTransport, setTransport } from "./bridge.ts";

const draft: Draft = {
  localPath: "  C:\\work\\proj  ",
  initialBrief: "  Build something real.  ",
};

// What the service actually committed. The brief is trimmed and the display name is derived from the
// workspace folder - neither value came from the submitted draft.
const committed: ProjectView = {
  projectId: "prj_abc123",
  name: "proj",
  localPath: "C:\\work\\proj",
  phase: "DISCOVERY",
  status: "ACTIVE",
  currentEpoch: 0,
  briefId: "brf_def456",
  briefVersion: 1,
  briefBody: "Build something real.",
  createdAt: "1700000000",
};

function submit(): IntakeState {
  return intakeReducer(intakeReducer(initialState, { type: "edit", draft }), { type: "submit" });
}

function createdProject(outcome: { kind: "created"; project: ProjectView } | never): ProjectView {
  return outcome.kind === "created" ? outcome.project : (() => { throw new Error("expected created"); })();
}

describe("intake state machine", () => {
  test("draft state holds unpersisted input and nothing authoritative", () => {
    const state = intakeReducer(initialState, { type: "edit", draft });
    assert.equal(state.kind, "editing");
    assert.equal(draftOf(state).initialBrief, draft.initialBrief);
    assert.ok(!("project" in state), "an editing state must carry no authoritative projection");
    assert.ok(hasDraft(state));
  });

  test("the draft carries no project name, because Rust derives it", () => {
    // A name in the draft would be a second source of identity metadata that could disagree with the
    // persisted value (DEC-050).
    assert.deepEqual(Object.keys(EMPTY_DRAFT).sort(), ["initialBrief", "localPath"]);
  });

  test("submitting retains the draft and carries no authoritative field", () => {
    const state = submit();
    assert.equal(state.kind, "submitting");
    assert.ok(isSubmitting(state));
    assert.equal(draftOf(state).initialBrief, draft.initialBrief, "the draft survives submission");
    assert.ok(!("project" in state), "a pending request must not show a project id, phase or epoch");
  });

  test("submit is ignored while already submitting, because create_project is not idempotent", () => {
    const once = submit();
    assert.equal(intakeReducer(once, { type: "submit" }), once, "a second submit must be a no-op");
  });

  test("submit is ignored after acceptance", () => {
    const created = intakeReducer(submit(), { type: "accepted", project: committed });
    assert.equal(intakeReducer(created, { type: "submit" }), created);
  });

  test("rejection retains the draft and carries the machine-readable code", () => {
    const error: CommandError = { code: "BLANK_INITIAL_BRIEF", message: "initial_brief.body must contain project intent" };
    const state = intakeReducer(submit(), { type: "rejected", error });
    assert.equal(state.kind, "rejected");
    assert.equal(draftOf(state).initialBrief, draft.initialBrief, "a refused request must not force the user to retype");
    assert.equal(state.kind === "rejected" ? state.error.code : "", "BLANK_INITIAL_BRIEF");
  });

  test("editing after a rejection returns to editing with the draft intact", () => {
    const rejected = intakeReducer(submit(), { type: "rejected", error: { code: "STORAGE_FAILURE", message: "disk" } });
    const edited = intakeReducer(rejected, { type: "edit", draft });
    assert.equal(edited.kind, "editing");
    assert.equal(draftOf(edited).initialBrief, draft.initialBrief);
  });
});

describe("authority boundary", () => {
  // THE test. A submitted draft and a committed project are constructed with deliberately different values.
  // If the reducer leaked any draft field into the accepted state, this fails.
  test("accepted state displays committed values rather than the submitted draft", () => {
    const state = intakeReducer(submit(), { type: "accepted", project: committed });
    assert.equal(state.kind, "created");
    assert.ok(!hasDraft(state), "an accepted state must not carry a draft");

    const project = state.kind === "created" ? state.project : undefined;
    assert.ok(project);
    assert.equal(project!.briefBody, "Build something real.", "display the stored brief, not '  Build something real.  '");
    assert.equal(project!.briefBody, project!.briefBody.trim(), "no field may retain submitted whitespace");
    assert.equal(project!.localPath, "C:\\work\\proj");
    assert.equal(project!.name, "proj", "the display name comes from the workspace folder, not the draft");
    assert.equal(project!.briefVersion, 1);
    assert.equal(project!.currentEpoch, 0);
  });

  test("the accepted state is built from the projection alone, even when it contradicts the draft", () => {
    // The audit's suggested mutation: the service returns a name that is not what was submitted anywhere. The
    // UI must show the returned value, proving it consumes authority rather than reconstructing from the form.
    const stored = { ...committed, name: "proj (stored)", phase: "DISCOVERY" };
    const state = intakeReducer(submit(), { type: "accepted", project: stored });
    const project = state.kind === "created" ? state.project : undefined;
    assert.equal(project!.name, "proj (stored)");
  });

  test("draftOf reports no draft once the service has committed", () => {
    const state = intakeReducer(submit(), { type: "accepted", project: committed });
    assert.deepEqual(draftOf(state), EMPTY_DRAFT);
  });
});

describe("transport boundary", () => {
  test("createProject returns the projection the service sent", async () => {
    resetTransport();
    setTransport(async () => committed);
    const result = await createProject({ local_path: "y", initial_brief: "z" });
    assert.deepEqual(result, committed);
    resetTransport();
  });

  test("createProject returns a rejection as a value, not a thrown error", async () => {
    resetTransport();
    setTransport(async () => {
      throw { code: "BLANK_INITIAL_BRIEF", message: "initial_brief.body must contain project intent" };
    });
    const result = await createProject({ local_path: "y", initial_brief: " " });
    assert.ok(!isProjectView(result));
    assert.equal((result as CommandError).code, "BLANK_INITIAL_BRIEF");
    resetTransport();
  });

  test("a non-contract rejection never becomes an optimistic success", async () => {
    resetTransport();
    setTransport(async () => {
      throw new Error("ipc channel closed");
    });
    const result = await createProject({ local_path: "y", initial_brief: "z" });
    assert.ok(!isProjectView(result));
    assert.equal((result as CommandError).code, "TRANSPORT_FAILURE");
    resetTransport();
  });

  test("the request sends no name, because the service derives the display name", async () => {
    // Rust takes camelCase Tauri arguments. If a name were sent it would either be ignored or reintroduce
    // caller-supplied identity metadata, which DEC-050 removed from the contract.
    resetTransport();
    let seen: { command: string; args: Record<string, unknown> } | null = null;
    setTransport(async (command, args) => {
      seen = { command, args };
      return committed;
    });
    await createProject({ local_path: "p", initial_brief: "b" });
    assert.equal(seen!.command, "create_project");
    assert.deepEqual(seen!.args, { localPath: "p", initialBrief: "b" });
    assert.ok(!("name" in seen!.args), "create_project must not carry a name argument");
    resetTransport();
  });
});

function isProjectView(value: unknown): value is ProjectView {
  return typeof (value as ProjectView)?.projectId === "string";
}