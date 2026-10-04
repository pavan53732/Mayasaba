// Tests for the intake state machine and the authority boundary.
//
// Run with: node --experimental-strip-types --test apps/desktop/src/intake/state.test.ts
//
// The load-bearing test is `accepted state displays committed values rather than the submitted draft`.
// Everything else supports it.

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
  name: "  My Project  ",
  localPath: "  C:\\work\\proj  ",
  initialBrief: "  Build something real.  ",
};

// What the service actually committed. Every field differs from the draft on purpose: whitespace is
// trimmed, and this is the only thing the UI is allowed to show after success.
const committed: ProjectView = {
  projectId: "prj_abc123",
  name: "My Project",
  localPath: "C:\\work\\proj",
  phase: "DISCOVERY",
  status: "ACTIVE",
  currentEpoch: 0,
  briefId: "brf_def456",
  briefVersion: 1,
  briefBody: "Build something real.",
};

function submit(): IntakeState {
  return intakeReducer(intakeReducer(initialState, { type: "edit", draft }), { type: "submit" });
}

describe("intake state machine", () => {
  test("draft state holds unpersisted input and nothing authoritative", () => {
    const state = intakeReducer(initialState, { type: "edit", draft });
    assert.equal(state.kind, "editing");
    assert.equal(draftOf(state).name, "  My Project  ");
    assert.ok(!("project" in state), "an editing state must carry no authoritative projection");
    assert.ok(hasDraft(state));
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
    const twice = intakeReducer(once, { type: "submit" });
    assert.equal(twice, once, "a second submit must be a no-op, not a second in-flight request");
  });

  test("submit is ignored after acceptance", () => {
    const created = intakeReducer(submit(), { type: "accepted", project: committed });
    assert.equal(intakeReducer(created, { type: "submit" }), created);
  });

  test("rejection retains the draft and carries the machine-readable code", () => {
    const error: CommandError = { code: "BLANK_INITIAL_BRIEF", message: "initial_brief.body must contain project intent" };
    const state = intakeReducer(submit(), { type: "rejected", error });
    assert.equal(state.kind, "rejected");
    assert.equal(draftOf(state).name, draft.name, "a refused request must not force the user to retype");
    assert.equal(state.kind === "rejected" ? state.error.code : "", "BLANK_INITIAL_BRIEF");
  });

  test("editing after a rejection returns to editing with the draft intact", () => {
    const rejected = intakeReducer(submit(), {
      type: "rejected",
      error: { code: "STORAGE_FAILURE", message: "disk" },
    });
    const edited = intakeReducer(rejected, { type: "edit", draft });
    assert.equal(edited.kind, "editing");
    assert.equal(draftOf(edited).name, draft.name);
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
    assert.equal(project!.name, "My Project", "display the stored name, not '  My Project  '");
    assert.equal(project!.name, project!.name.trim(), "no field may retain submitted whitespace");
    assert.equal(project!.localPath, "C:\\work\\proj");
    assert.equal(project!.briefBody, "Build something real.");
    assert.equal(project!.briefVersion, 1);
    assert.equal(project!.currentEpoch, 0);
  });

  test("the accepted state is built from the projection alone, even when it contradicts the draft", () => {
    // The audit's suggested mutation: the service returns a name that is not what was submitted. The UI must
    // show the returned name. This is what proves the UI is consuming authority rather than reconstructing
    // its own project object from the form.
    const stored = { ...committed, name: "My Project (stored)", phase: "DISCOVERY" };
    const state = intakeReducer(submit(), { type: "accepted", project: stored });

    const project = state.kind === "created" ? state.project : undefined;
    assert.equal(project!.name, "My Project (stored)");
    assert.notEqual(project!.name, draft.name.trim(), "the submitted value must not resurface");
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
    const result = await createProject({ name: "x", local_path: "y", initial_brief: "z" });
    assert.deepEqual(result, committed);
    resetTransport();
  });

  test("createProject returns a rejection as a value, not a thrown error", async () => {
    resetTransport();
    setTransport(async () => {
      throw { code: "BLANK_INITIAL_BRIEF", message: "initial_brief.body must contain project intent" };
    });
    const result = await createProject({ name: "x", local_path: "y", initial_brief: " " });
    assert.ok(!isProjectView(result));
    assert.equal((result as CommandError).code, "BLANK_INITIAL_BRIEF");
    resetTransport();
  });

  test("a non-contract rejection never becomes an optimistic success", async () => {
    // If Rust ever throws something that is not a CommandError, the UI must not treat it as a created
    // project. Inventing a success here is how a UI ends up displaying authority it never received.
    resetTransport();
    setTransport(async () => {
      throw new Error("ipc channel closed");
    });
    const result = await createProject({ name: "x", local_path: "y", initial_brief: "z" });
    assert.ok(!isProjectView(result));
    assert.equal((result as CommandError).code, "TRANSPORT_FAILURE");
    resetTransport();
  });

  test("the request reaches the command with the argument names Rust expects", async () => {
    // Rust takes camelCase Tauri arguments; the declared payload is snake_case. A mismatch here would be a
    // silent field-name bug, so the mapping is asserted rather than assumed.
    resetTransport();
    let seen: { command: string; args: Record<string, unknown> } | null = null;
    setTransport(async (command, args) => {
      seen = { command, args };
      return committed;
    });
    await createProject({ name: "n", local_path: "p", initial_brief: "b" });
    assert.equal(seen!.command, "create_project");
    assert.deepEqual(seen!.args, { name: "n", localPath: "p", initialBrief: "b" });
    resetTransport();
  });
});

function isProjectView(value: unknown): value is ProjectView {
  return typeof (value as ProjectView)?.projectId === "string";
}