// Tests for the workspace selection state machine.
//
// The load-bearing property: selecting a folder is not authorizing it. A candidate must never enable Create,
// and only a Rust answer may produce the authorized state.

import assert from "node:assert/strict";
import { test, describe } from "node:test";

import {
  emptyWorkspace,
  isAuthorized,
  workspaceReducer,
  type WorkspaceState,
} from "./state.ts";

const selected = workspaceReducer(emptyWorkspace, { type: "selected", path: "C:\\work\\proj" });

describe("workspace selection is not authorization", () => {
  test("a selected folder is only a candidate", () => {
    assert.equal(selected.kind, "candidate");
    assert.ok(!isAuthorized(selected), "selecting a folder must not authorize it");
  });

  test("a typed path is only a candidate", () => {
    const typed = workspaceReducer(emptyWorkspace, { type: "edit", requestedPath: "C:\\typed" });
    assert.equal(typed.kind, "candidate");
    assert.ok(!isAuthorized(typed));
  });

  test("only a Rust answer produces the authorized state", () => {
    const authorized = workspaceReducer(selected, {
      type: "authorized",
      canonicalPath: "C:\\work\\proj",
    });
    assert.equal(authorized.kind, "authorized");
    assert.ok(isAuthorized(authorized));
    // The canonical path is what gets persisted, and it is Rust's value, not the UI's spelling.
    assert.equal(authorized.kind === "authorized" ? authorized.canonicalPath : "", "C:\\work\\proj");
  });

  test("a rejection is invalid, never authorized", () => {
    const rejected = workspaceReducer(selected, {
      type: "rejected",
      code: "WORKSPACE_DOES_NOT_EXIST",
      message: "That folder does not exist.",
    });
    assert.equal(rejected.kind, "invalid");
    assert.ok(!isAuthorized(rejected), "a rejected workspace must not satisfy the create requirement");
    assert.equal(rejected.kind === "invalid" ? rejected.code : "", "WORKSPACE_DOES_NOT_EXIST");
  });

  test("canonicalization from Rust replaces the candidate spelling", () => {
    // The UI sent "C:\\work\\..\\work\\proj"; Rust answered with the canonical root. That value is authoritative.
    const candidate = workspaceReducer(emptyWorkspace, { type: "edit", requestedPath: "C:\\work\\..\\work\\proj" });
    const authorized = workspaceReducer(candidate, { type: "authorized", canonicalPath: "C:\\work\\proj" });
    assert.equal(authorized.kind === "authorized" ? authorized.requestedPath : "", "C:\\work\\..\\work\\proj");
    assert.equal(authorized.kind === "authorized" ? authorized.canonicalPath : "", "C:\\work\\proj");
  });
});

describe("workspace picker lifecycle", () => {
  test("browse opens the picker and a second browse is ignored while it is open", () => {
    const selecting = workspaceReducer(emptyWorkspace, { type: "browse" });
    assert.equal(selecting.kind, "selecting");
    assert.equal(workspaceReducer(selecting, { type: "browse" }), selecting, "a second dialog must not open");
  });

  test("cancelling from a fresh browse leaves nothing selected", () => {
    const cancelled = workspaceReducer(
      workspaceReducer(emptyWorkspace, { type: "browse" }),
      { type: "cancelled" },
    );
    assert.equal(cancelled.kind, "empty");
    assert.ok(!isAuthorized(cancelled));
  });

  test("cancelling a later browse keeps an already authorized workspace", () => {
    // An abandoned browse must not discard a workspace the user already chose.
    const authorized = workspaceReducer(selected, { type: "authorized", canonicalPath: "C:\\work\\proj" });
    const afterCancel = workspaceReducer(
      workspaceReducer(authorized, { type: "browse" }),
      { type: "cancelled" },
    );
    assert.equal(afterCancel.kind, "authorized");
  });

  test("clearing manual entry returns to empty rather than a stale candidate", () => {
    const typed = workspaceReducer(emptyWorkspace, { type: "edit", requestedPath: "C:\\typed" });
    const cleared = workspaceReducer(typed, { type: "edit", requestedPath: "   " });
    assert.equal(cleared.kind, "empty");
  });

  test("every non-authorized state fails the create requirement", () => {
    const states: WorkspaceState[] = [
      emptyWorkspace,
      { kind: "selecting", retain: null },
      selected,
      { kind: "invalid", requestedPath: "x", code: "WORKSPACE_EMPTY", message: "no" },
    ];
    for (const state of states) {
      assert.ok(!isAuthorized(state), `${state.kind} must not enable Create`);
    }
  });
});