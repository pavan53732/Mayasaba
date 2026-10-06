// Tests for the typed bridge wrappers over the M2.5 shell surface.
//
// Run with: npm test
//
// The load-bearing property is the one DEC-054 exists for: every wrapper sends the wire names the contract
// declares, snake_case, and nothing translates them. A wrapper that sent `projectId` instead of `project_id`
// would compile, typecheck, and then fail at runtime inside Tauri - the argument names are not part of the
// TypeScript type of `Transport`, so only a test can hold them.
//
// The second property is that a refusal is returned and never turned into a success. A UI that rendered a
// replay as done because a wrapper had swallowed a rejection would be displaying authority it never received.

import assert from "node:assert/strict";
import { afterEach, describe, test } from "node:test";

import {
  attachProjectContextAttachment,
  getCommunicationHealth,
  getEventCursor,
  listProjectContextAttachments,
  replayDeadLetter,
  resetTransport,
  resolveProjectContextAttachment,
  setTransport,
  type CommunicationHealth,
  type EventCursor,
  type ReplayOutcome,
} from "./bridge.ts";

afterEach(() => resetTransport());

type Call = { command: string; args: Record<string, unknown> };

/** A transport that records what it was asked and answers with `result`. */
function recording(result: unknown): Call[] {
  const calls: Call[] = [];
  setTransport(async (command, args) => {
    calls.push({ command, args });
    return result;
  });
  return calls;
}

/** A transport that rejects with `thrown`. */
function rejecting(thrown: unknown): void {
  setTransport(async () => {
    throw thrown;
  });
}

describe("wire names", () => {
  test("get_communication_health sends project_id, snake_case", async () => {
    const calls = recording({ project_id: "prj_a" });
    await getCommunicationHealth("prj_a");

    assert.equal(calls.length, 1);
    const call = calls[0]!;
    assert.equal(call.command, "get_communication_health");
    // The exact key set, not merely the presence of the right one: an extra camelCase alias would be a second
    // spelling of one field travelling on the same wire.
    assert.deepEqual(Object.keys(call.args), ["project_id"]);
    assert.equal(call.args.project_id, "prj_a");
  });

  test("get_event_cursor sends project_id and consumer_id, snake_case", async () => {
    const calls = recording({ project_id: "prj_a" });
    await getEventCursor("prj_a", "control_room");

    const call = calls[0]!;
    assert.equal(call.command, "get_event_cursor");
    assert.deepEqual(Object.keys(call.args), ["project_id", "consumer_id"]);
    assert.equal(call.args.project_id, "prj_a");
    assert.equal(call.args.consumer_id, "control_room");
  });

  test("replay_dead_letter sends message_id, snake_case", async () => {
    const calls = recording({ replayed_message_id: "msg_new" });
    await replayDeadLetter("msg_dead");

    const call = calls[0]!;
    assert.equal(call.command, "replay_dead_letter");
    assert.deepEqual(Object.keys(call.args), ["message_id"]);
    assert.equal(call.args.message_id, "msg_dead");
  });
});

describe("the answer is Rust's", () => {
  test("a health response is returned field for field, untranslated", async () => {
    const health: CommunicationHealth = {
      project_id: "prj_a",
      backlog: 3,
      enqueue_limit: 1024,
      counts: { queued: 1, retrying: 1, expired: 0, dead_lettered: 1 },
      open_gaps: [
        { code: "SEQUENCE_GAP", session_id: "sess_1", channel: "task", expected: 2, found: 4 },
      ],
      transport_status: "not_connected",
    };
    recording(health);

    // Identity, not equality: the wrapper must not add, drop, default or rename a single field on the way
    // through, because every one of those would be the UI holding a different answer than Rust committed.
    assert.equal(await getCommunicationHealth("prj_a"), health);
  });

  test("a cursor with no durable derivation stays null rather than becoming a number", async () => {
    const cursor: EventCursor = {
      project_id: "prj_a",
      consumer_id: "control_room",
      last_sequence: 0,
      next_sequence: null,
      gap_detected: false,
      resync_from: null,
    };
    recording(cursor);

    const result = (await getEventCursor("prj_a", "control_room")) as EventCursor;
    assert.equal(result.next_sequence, null);
    assert.equal(result.resync_from, null);
  });

  test("a replay outcome carries the original identity and context_refreshed false", async () => {
    const outcome: ReplayOutcome = {
      replayed_message_id: "msg_new",
      source_message_id: "msg_dead",
      context_snapshot_id: "ctx_original",
      state_digest: "a3f1c8e07b2d4956af13c8e07b2d4956af13c8e07b2d4956af13c8e07b2d4956",
      context_refreshed: false,
      deduplicated: false,
    };
    recording(outcome);

    const result = (await replayDeadLetter("msg_dead")) as ReplayOutcome;
    assert.equal(result.context_refreshed, false);
    assert.equal(result.source_message_id, "msg_dead");
    assert.notEqual(result.replayed_message_id, result.source_message_id);
  });
});

describe("a refusal is returned, never rendered as success", () => {
  test("an authorization refusal keeps its registered code", async () => {
    // The replay path fails closed on a material action, and the code is what the UI acts on. Replacing it with
    // a generic message would erase the difference between "not allowed" and "not yet".
    rejecting({
      code: "AUTHORIZATION_NOT_IMPLEMENTED",
      message: "msg_accept is a TASK_ACCEPT, which is a material action",
    });

    const result = (await replayDeadLetter("msg_accept")) as { code: string };
    assert.equal(result.code, "AUTHORIZATION_NOT_IMPLEMENTED");
  });

  test("a schema-invalid refusal keeps its code", async () => {
    rejecting({ code: "SCHEMA_INVALID", message: "messages.message_id = msg_absent" });

    const result = (await replayDeadLetter("msg_absent")) as { code: string };
    assert.equal(result.code, "SCHEMA_INVALID");
  });

  test("a non-CommandError rejection becomes TRANSPORT_FAILURE, not a value", async () => {
    rejecting(new Error("the IPC channel closed"));

    const result = (await getCommunicationHealth("prj_a")) as { code: string; message: string };
    assert.equal(result.code, "TRANSPORT_FAILURE");
    assert.match(result.message, /IPC channel closed/);
  });

  test("a thrown non-Error is still reported as a failure", async () => {
    rejecting("not an error object");

    const result = (await getEventCursor("prj_a", "ui")) as { code: string; message: string };
    assert.equal(result.code, "TRANSPORT_FAILURE");
    assert.equal(result.message, "not an error object");
  });

  test("a refusal carries no success fields, so it cannot be read as a completed replay", async () => {
    rejecting({ code: "AUTHORIZATION_NOT_IMPLEMENTED", message: "refused" });

    // Through `unknown` deliberately: the point of the assertion is that the returned value does *not* have the
    // success shape, so it cannot be reached by casting to that shape.
    const result = (await replayDeadLetter("msg_accept")) as unknown as Record<string, unknown>;
    assert.equal(result.replayed_message_id, undefined);
    assert.equal(result.context_refreshed, undefined);
    assert.equal(result.deduplicated, undefined);
  });
});

describe("attachment operations send the declared wire names", () => {
  test("attach sends project_id, source_path and provenance", async () => {
    const calls = recording({ attachment: {}, verdict: "RESOLVED", checks: [] });
    await attachProjectContextAttachment(
      "prj_a",
      "C:\\work\\proj\\src\\main.rs",
      "CHAT_COMPOSER",
    );

    const call = calls[0]!;
    assert.equal(call.command, "attach_project_context_attachment");
    assert.deepEqual(Object.keys(call.args), ["project_id", "source_path", "provenance"]);
    assert.equal(call.args.project_id, "prj_a");
    assert.equal(call.args.source_path, "C:\\work\\proj\\src\\main.rs");
    assert.equal(call.args.provenance, "CHAT_COMPOSER");
    // The path is sent exactly as the user's selection, not pre-normalized here. Canonicalization is the
    // service's, and a second normalization on this side would be a second answer to what the path is.
    assert.notEqual(call.args.source_path, "\\\\?\\C:\\work\\proj\\src\\main.rs");
  });

  test("attach sends no scope argument, so this side cannot choose the boundary", async () => {
    // DEC-048. The scope is read from the project by the service; a wire field for it would let the caller pick
    // the boundary its own path is validated against.
    const calls = recording({ attachment: {}, verdict: "RESOLVED", checks: [] });
    await attachProjectContextAttachment("prj_a", "C:\\work\\proj", "INITIAL_INTAKE_COMPOSER");

    assert.deepEqual(Object.keys(calls[0]!.args), ["project_id", "source_path", "provenance"]);
  });

  test("list sends project_id alone", async () => {
    const calls = recording([]);
    await listProjectContextAttachments("prj_a");

    const call = calls[0]!;
    assert.equal(call.command, "list_project_context_attachments");
    assert.deepEqual(Object.keys(call.args), ["project_id"]);
  });

  test("resolve sends project_id and attachment_id", async () => {
    const calls = recording({ attachment: {}, verdict: "UNRESOLVED", checks: [] });
    await resolveProjectContextAttachment("prj_a", "att_1");

    const call = calls[0]!;
    assert.equal(call.command, "resolve_project_context_attachment");
    assert.deepEqual(Object.keys(call.args), ["project_id", "attachment_id"]);
    assert.equal(call.args.attachment_id, "att_1");
  });

  test("an attachment refusal keeps its registered code", async () => {
    // A path outside the authorized workspace is refused by WorkspaceService, and the code is what the tray
    // renders. Collapsing it into a generic failure would erase the difference between "outside your workspace"
    // and "the file is gone".
    rejecting({ code: "ATTACHMENT_NOT_IN_SCOPE", message: "outside the authorized workspace" });

    const result = (await attachProjectContextAttachment("prj_a", "D:\\elsewhere", "CHAT_COMPOSER")) as {
      code: string;
    };
    assert.equal(result.code, "ATTACHMENT_NOT_IN_SCOPE");
  });

  test("a missing attachment is not rendered as a stored one", async () => {
    rejecting({ code: "ATTACHMENT_NOT_FOUND", message: "no such attachment" });

    const result = (await resolveProjectContextAttachment("prj_a", "att_absent")) as unknown as Record<
      string,
      unknown
    >;
    assert.equal(result.code, "ATTACHMENT_NOT_FOUND");
    assert.equal(result.attachment, undefined);
    assert.equal(result.verdict, undefined);
  });
});
