// Tests for the shared attachment presentation state.
//
// The load-bearing property: selecting a file is not attaching it, and attaching it is not capturing,
// indexing or accepting it. Only an answer from Rust may make an entry durable, and the lifecycle, the
// resolvability verdict, the hash and the evidence link are read off that answer rather than derived.
//
// These are the UI half of the tests DEC-106 lists. The backend half is in
// crates/core/tests/attachment_slice.rs and crates/workspace/tests/attachment_validation.rs.

import assert from "node:assert/strict";
import { test, describe } from "node:test";

import {
  emptyTray,
  isDurable,
  presentationOf,
  queuedPaths,
  durableEntries,
  trayReducer,
  type AttachmentCheck,
  type AttachmentResolution,
  type AttachmentView,
} from "./state.ts";

const USER_SPELLING = "C:\\work\\proj\\src\\main.rs";
// What Rust actually stores: the canonical path, verbatim prefix and all. It is deliberately a different
// string from the user's spelling, so a test can tell which one the UI rendered.
const CANONICAL = "\\\\?\\C:\\work\\proj\\src\\main.rs";

function resolvedChecks(): AttachmentCheck[] {
  return [
    { check: "EXISTS", status: "PASS", code: null, detail: null },
    { check: "LOCALITY", status: "PASS", code: null, detail: null },
    { check: "KIND", status: "PASS", code: null, detail: null },
    { check: "SCOPE", status: "PASS", code: null, detail: null },
  ];
}

function missingChecks(): AttachmentCheck[] {
  return [
    {
      check: "EXISTS",
      status: "FAIL",
      code: "ATTACHMENT_SOURCE_MISSING",
      detail: "nothing exists at the selected attachment path",
    },
    { check: "LOCALITY", status: "NOT_APPLICABLE", code: null, detail: null },
    { check: "KIND", status: "NOT_APPLICABLE", code: null, detail: null },
    { check: "SCOPE", status: "NOT_APPLICABLE", code: null, detail: null },
  ];
}

function resolution(
  overrides: Partial<AttachmentView> = {},
  verdict: AttachmentResolution["verdict"] = "RESOLVED",
  checks: AttachmentCheck[] = resolvedChecks(),
): AttachmentResolution {
  return {
    attachment: {
      attachment_id: "att_9c1f4a7b2e5d8031",
      project_id: "prj_2f1c9a4b6e0d3857",
      source_path: CANONICAL,
      kind: "FILE",
      authorized_scope: "\\\\?\\C:\\work\\proj",
      provenance: "CHAT_COMPOSER",
      lifecycle_state: "SELECTED",
      content_hash: null,
      context_evidence_id: null,
      captured_at: "1700000000",
      ...overrides,
    },
    verdict,
    checks,
  };
}

const selected = trayReducer(emptyTray("CHAT_COMPOSER"), { type: "selected", path: USER_SPELLING });

describe("selecting a path is not attaching it", () => {
  test("a selected path is a candidate and nothing is durable", () => {
    assert.equal(selected.entries.length, 1);
    assert.equal(selected.entries[0].kind, "candidate");
    assert.equal(durableEntries(selected).length, 0);
    assert.ok(!isDurable(selected.entries[0]));
  });

  test("an in-flight attach is still not durable", () => {
    const attaching = trayReducer(selected, { type: "attaching", requestedPath: USER_SPELLING });
    assert.equal(attaching.entries[0].kind, "attaching");
    assert.equal(durableEntries(attaching).length, 0);
  });

  test("only Rust's answer makes an entry durable", () => {
    const attached = trayReducer(selected, {
      type: "attached",
      requestedPath: USER_SPELLING,
      resolution: resolution(),
    });
    assert.equal(attached.entries[0].kind, "attached");
    assert.equal(durableEntries(attached).length, 1);
  });
});

describe("the UI never claims more than the controller returned", () => {
  test("an unrecorded entry reports no lifecycle, verdict, hash or evidence link", () => {
    for (const entry of [
      selected.entries[0],
      trayReducer(selected, { type: "attaching", requestedPath: USER_SPELLING }).entries[0],
      trayReducer(selected, {
        type: "refused",
        requestedPath: USER_SPELLING,
        code: "ATTACHMENT_NOT_IN_SCOPE",
        message: "outside the authorized workspace",
      }).entries[0],
    ]) {
      const shown = presentationOf(entry);
      assert.equal(shown.durable, false);
      assert.equal(shown.recordedState, null);
      assert.equal(shown.verdict, null);
      assert.equal(shown.captured, false);
      assert.equal(shown.consumed, false);
      assert.equal(shown.provenance, null);
      assert.deepEqual(shown.checks, []);
    }
  });

  test("the recorded provenance is Rust's, so both surfaces are distinguishable", () => {
    // The project's attachment set is project-scoped, not provenance-scoped, so one list can hold references
    // from both composers. Each entry displays the origin Rust recorded rather than the surface it is shown on.
    for (const provenance of ["INITIAL_INTAKE_COMPOSER", "CHAT_COMPOSER"] as const) {
      const attached = trayReducer(selected, {
        type: "attached",
        requestedPath: USER_SPELLING,
        resolution: resolution({ provenance }),
      });
      assert.equal(presentationOf(attached.entries[0]).provenance, provenance);
    }
  });

  test("the displayed path is Rust's canonical path, never the user's spelling", () => {
    const attached = trayReducer(selected, {
      type: "attached",
      requestedPath: USER_SPELLING,
      resolution: resolution(),
    });
    const shown = presentationOf(attached.entries[0]);
    assert.equal(shown.path, CANONICAL);
    assert.notEqual(shown.path, USER_SPELLING);
  });

  test("the recorded lifecycle is Rust's value, including states the UI cannot produce", () => {
    // The UI has no transition that yields ACCEPTED or PENDING. They can only arrive from Rust, and they are
    // displayed as received rather than mapped onto a friendlier local vocabulary.
    for (const state of ["SELECTED", "PENDING", "ACCEPTED", "REJECTED"] as const) {
      const attached = trayReducer(selected, {
        type: "attached",
        requestedPath: USER_SPELLING,
        resolution: resolution({ lifecycle_state: state }),
      });
      assert.equal(presentationOf(attached.entries[0]).recordedState, state);
    }
  });

  test("captured is true only when the row carries a content hash", () => {
    // An accepted lifecycle is not a capture. DEC-106 makes capture a separate explicit operation, and only it
    // sets content_hash, so an ACCEPTED row with no hash must not render as captured or indexed.
    const acceptedNoHash = trayReducer(selected, {
      type: "attached",
      requestedPath: USER_SPELLING,
      resolution: resolution({ lifecycle_state: "ACCEPTED", content_hash: null }),
    });
    assert.equal(presentationOf(acceptedNoHash.entries[0]).captured, false);

    const captured = trayReducer(selected, {
      type: "attached",
      requestedPath: USER_SPELLING,
      resolution: resolution({ content_hash: "a3f1c8e07b2d4956" }),
    });
    assert.equal(presentationOf(captured.entries[0]).captured, true);
  });

  test("consumed is true only when the row carries an evidence link", () => {
    const notConsumed = trayReducer(selected, {
      type: "attached",
      requestedPath: USER_SPELLING,
      resolution: resolution({ lifecycle_state: "ACCEPTED" }),
    });
    assert.equal(presentationOf(notConsumed.entries[0]).consumed, false);

    const consumed = trayReducer(selected, {
      type: "attached",
      requestedPath: USER_SPELLING,
      resolution: resolution({ context_evidence_id: "evd_4b7c1e9a" }),
    });
    assert.equal(presentationOf(consumed.entries[0]).consumed, true);
  });

  test("an unresolved reference is displayed as attached but no longer resolvable", () => {
    // The row survives a deleted source, so the entry is durable and the verdict is the observation.
    const gone = trayReducer(selected, {
      type: "attached",
      requestedPath: USER_SPELLING,
      resolution: resolution({}, "UNRESOLVED", missingChecks()),
    });
    const shown = presentationOf(gone.entries[0]);
    assert.equal(shown.durable, true);
    assert.equal(shown.verdict, "UNRESOLVED");
    assert.equal(shown.checks[0].status, "FAIL");
    assert.equal(shown.checks[0].code, "ATTACHMENT_SOURCE_MISSING");
    // The checks that were never reached are not reported as passing.
    assert.equal(shown.checks[1].status, "NOT_APPLICABLE");
  });
});

describe("durable references are not removable from the UI", () => {
  test("an unrecorded selection can be discarded", () => {
    const discarded = trayReducer(selected, { type: "discarded", requestedPath: USER_SPELLING });
    assert.equal(discarded.entries.length, 0);
  });

  test("a recorded reference survives a discard", () => {
    // Rows are never deleted and source_path is never rewritten (DEC-106). A tray that could drop an attached
    // entry would let the UI show stored state as absent, so there is no action that removes one.
    const attached = trayReducer(selected, {
      type: "attached",
      requestedPath: USER_SPELLING,
      resolution: resolution(),
    });
    const attempted = trayReducer(attached, { type: "discarded", requestedPath: USER_SPELLING });
    assert.equal(durableEntries(attempted).length, 1);
  });

  test("a refusal keeps the entry and the spelling the user used", () => {
    const refused = trayReducer(selected, {
      type: "refused",
      requestedPath: USER_SPELLING,
      code: "ATTACHMENT_NOT_IN_SCOPE",
      message: "outside the authorized workspace",
    });
    const shown = presentationOf(refused.entries[0]);
    assert.equal(shown.durable, false);
    assert.equal(shown.path, USER_SPELLING);
    assert.equal(shown.label, "Refused (ATTACHMENT_NOT_IN_SCOPE)");
  });

  test("starting a new intake clears the tray, because it is about a different project", () => {
    // The bug this covers: the intake composer begins a new project after one is created, and without this the
    // second composer would render the first project's references. A tray describes one project's context.
    const attached = trayReducer(selected, {
      type: "attached",
      requestedPath: USER_SPELLING,
      resolution: resolution(),
    });
    const cleared = trayReducer(attached, { type: "cleared" });
    assert.equal(cleared.entries.length, 0);
    // Only the entries change. The provenance survives, because the surface is still the same surface.
    assert.equal(cleared.provenance, attached.provenance);
  });
});

describe("rehydration shows what Rust stores", () => {
  test("the durable portion is replaced and unrecorded selections survive", () => {
    const withCandidate = trayReducer(selected, { type: "selected", path: "C:\\work\\proj\\README.md" });
    const attached = trayReducer(withCandidate, {
      type: "attached",
      requestedPath: USER_SPELLING,
      resolution: resolution(),
    });

    // Rust now reports one row, and it is not the one the UI was holding: a different attachment identity.
    const rehydrated = trayReducer(attached, {
      type: "rehydrated",
      resolutions: [resolution({ attachment_id: "att_stored" })],
    });

    const durable = durableEntries(rehydrated);
    assert.equal(durable.length, 1);
    assert.equal(
      durable[0].kind === "attached" ? durable[0].resolution.attachment.attachment_id : "",
      "att_stored",
    );
    // The unrecorded selection was never durable, so rehydration does not discard it.
    assert.deepEqual(queuedPaths(rehydrated), ["C:\\work\\proj\\README.md"]);
  });

  test("queuedPaths returns only unrecorded selections", () => {
    // The intake path attaches this list after create_project returns a project id. An entry that is already
    // recorded must not appear, or a retry would record a second identity for one selection.
    const tray = trayReducer(
      trayReducer(selected, {
        type: "attached",
        requestedPath: USER_SPELLING,
        resolution: resolution(),
      }),
      { type: "selected", path: "C:\\work\\proj\\docs" },
    );
    assert.deepEqual(queuedPaths(tray), ["C:\\work\\proj\\docs"]);
  });
});

describe("the tray names the surface that offered the reference", () => {
  test("provenance is per tray and is carried unchanged into the request", () => {
    const intake = emptyTray("INITIAL_INTAKE_COMPOSER");
    const chat = emptyTray("CHAT_COMPOSER");
    assert.equal(intake.provenance, "INITIAL_INTAKE_COMPOSER");
    assert.equal(chat.provenance, "CHAT_COMPOSER");
  });
});
