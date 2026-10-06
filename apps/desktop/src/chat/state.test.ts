// Tests for the Ongoing Chat Composer.
//
// The load-bearing property, stated by DEC-106 and required on both surfaces: an attachment is never a
// prerequisite for submitting a normal user message. The first test below is the one DEC-106 names - "the chat
// composer submits a normal message with no attachment" - and the rest of the file is the converse and the
// boundary around it.

import assert from "node:assert/strict";
import { test, describe } from "node:test";

import { emptyTray, trayReducer, type AttachmentTray } from "../attachments/state.ts";
import {
  canSendMessage,
  chatReducer,
  chatTray,
  initialChatState,
  isSending,
  type ChatState,
  type UserContribution,
} from "./state.ts";

function editing(text: string, tray: AttachmentTray = emptyTray("CHAT_COMPOSER")): ChatState {
  return { kind: "editing", text, tray };
}

/** A tray holding one recorded but no-longer-resolvable reference. */
function unresolvedTray(): AttachmentTray {
  const selected = trayReducer(emptyTray("CHAT_COMPOSER"), {
    type: "selected",
    path: "C:\\work\\proj\\gone.txt",
  });
  return trayReducer(selected, {
    type: "attached",
    requestedPath: "C:\\work\\proj\\gone.txt",
    resolution: {
      attachment: {
        attachment_id: "att_gone",
        project_id: "prj_1",
        source_path: "\\\\?\\C:\\work\\proj\\gone.txt",
        kind: "FILE",
        authorized_scope: "\\\\?\\C:\\work\\proj",
        provenance: "CHAT_COMPOSER",
        lifecycle_state: "SELECTED",
        content_hash: null,
        context_evidence_id: null,
        captured_at: "1700000000",
      },
      verdict: "UNRESOLVED",
      checks: [
        {
          check: "EXISTS",
          status: "FAIL",
          code: "ATTACHMENT_SOURCE_MISSING",
          detail: "nothing exists at the selected attachment path",
        },
      ],
    },
  });
}

/** A tray holding a reference Rust refused outright. */
function refusedTray(): AttachmentTray {
  const selected = trayReducer(emptyTray("CHAT_COMPOSER"), {
    type: "selected",
    path: "D:\\elsewhere\\outside.txt",
  });
  return trayReducer(selected, {
    type: "refused",
    requestedPath: "D:\\elsewhere\\outside.txt",
    code: "ATTACHMENT_NOT_IN_SCOPE",
    message: "the selected path is outside the authorized workspace",
  });
}

describe("a normal message needs no attachment", () => {
  test("the chat composer submits a normal message with no attachment", () => {
    const state = editing("Please look at the failing build.");
    assert.equal(chatTray(state).entries.length, 0);
    assert.equal(canSendMessage(state), true);
  });

  test("an attachment is never a prerequisite: the tray cannot change the answer", () => {
    // The predicate takes no tray, so this compares the only thing that could have gated it. Four trays in
    // four different conditions, one text, one answer.
    const text = "Please look at the failing build.";
    const trays = [emptyTray("CHAT_COMPOSER"), unresolvedTray(), refusedTray()];
    const answers = trays.map((tray) => canSendMessage(editing(text, tray)));
    assert.deepEqual(answers, [true, true, true]);
  });

  test("an unresolved attachment does not block the message", () => {
    // A moved file is an observation about a reference, not a reason to stop the user talking (DEC-106).
    const state = editing("The file moved, but here is what I meant.", unresolvedTray());
    assert.equal(canSendMessage(state), true);
  });

  test("a refused attachment does not block the message", () => {
    const state = editing("That path was outside the workspace anyway.", refusedTray());
    assert.equal(canSendMessage(state), true);
  });
});

describe("attachment is not a substitute prerequisite either", () => {
  test("attachments alone do not make an empty message sendable", () => {
    // The converse of the rule. If a tray could enable sending, the composer would be accepting a message that
    // carries no user text, which is not a contribution the service would record: `body` is required and text
    // that is whitespace only is refused before anything is written.
    for (const tray of [unresolvedTray(), refusedTray()]) {
      assert.equal(canSendMessage(editing("", tray)), false);
      assert.equal(canSendMessage(editing("   ", tray)), false);
    }
  });

  test("whitespace is not a message", () => {
    assert.equal(canSendMessage(editing("")), false);
    assert.equal(canSendMessage(editing("\n\t ")), false);
  });
});

describe("the composer holds the draft through failure", () => {
  test("a double submit is guarded", () => {
    const sending = chatReducer(editing("hello"), { type: "submit" });
    assert.ok(isSending(sending));
    assert.equal(chatReducer(sending, { type: "submit" }), sending);
  });

  test("a refusal keeps the draft and the tray", () => {
    const before = editing("please retry", unresolvedTray());
    const refused = chatReducer(before, {
      type: "refused",
      error: { code: "TRANSPORT_FAILURE", message: "the shell is not running" },
    });
    assert.equal(refused.kind, "refused");
    assert.equal(refused.text, "please retry");
    assert.equal(chatTray(refused).entries.length, 1);
    assert.equal(canSendMessage(refused), true);
  });

  test("a sending state is not sendable again", () => {
    assert.equal(canSendMessage(chatReducer(editing("hello"), { type: "submit" })), false);
  });
});

describe("both surfaces share one attachment reducer", () => {
  test("the chat composer delegates tray actions rather than re-implementing them", () => {
    // One reducer, two surfaces. A chat-specific copy of these rules is how the two surfaces would come to
    // disagree about what "attached" means.
    const selected = chatReducer(initialChatState("CHAT_COMPOSER"), {
      type: "attachment",
      action: { type: "selected", path: "C:\\work\\proj\\notes.md" },
    });
    assert.equal(chatTray(selected).entries[0].kind, "candidate");
    assert.equal(chatTray(selected).provenance, "CHAT_COMPOSER");

    const attached = chatReducer(selected, {
      type: "attachment",
      action: {
        type: "attached",
        requestedPath: "C:\\work\\proj\\notes.md",
        resolution: {
          attachment: {
            attachment_id: "att_notes",
            project_id: "prj_1",
            source_path: "\\\\?\\C:\\work\\proj\\notes.md",
            kind: "FILE",
            authorized_scope: "\\\\?\\C:\\work\\proj",
            provenance: "CHAT_COMPOSER",
            lifecycle_state: "SELECTED",
            content_hash: null,
            context_evidence_id: null,
            captured_at: "1700000000",
          },
          verdict: "RESOLVED",
          checks: [],
        },
      },
    });
    assert.equal(chatTray(attached).entries[0].kind, "attached");
    // Editing text does not disturb the tray, and a tray action does not disturb the text.
    assert.equal(chatReducer(attached, { type: "edit", text: "hi" }).text, "hi");
    assert.equal(chatTray(chatReducer(attached, { type: "edit", text: "hi" })).entries.length, 1);
  });
});

describe("a recorded contribution is the row the service stored", () => {
  /** The stored row, in the shape `record_user_contributionResponse` declares. */
  const stored: UserContribution = {
    contribution_id: "con_0000000000000001",
    project_id: "prj_1",
    body: "please also cover the export path",
    classification: "COMMENTARY",
    classification_confidence: null,
    classification_source: "INTAKE_ROUTER",
    result_type: "PENDING",
    result_reference: null,
    epoch_before: 0,
    epoch_after: 0,
    created_at: "1700000000",
  };

  /** Narrow a state to its contribution, so the assertions below read as the property rather than as a cast. */
  function recordedOf(state: ChatState): UserContribution {
    assert.equal(state.kind, "recorded");
    if (state.kind !== "recorded") throw new Error("the state was not recorded");
    return state.contribution;
  }

  test("recording clears the draft so one intent cannot be recorded twice", () => {
    const sending = chatReducer(editing("please also cover the export path"), { type: "submit" });
    const recorded = chatReducer(sending, { type: "recorded", contribution: stored });
    assert.equal(recorded.kind, "recorded");
    assert.equal(recorded.text, "");
    // An empty draft is not sendable, so the same intent cannot be submitted again from this state.
    assert.equal(canSendMessage(recorded), false);
  });

  test("the composer renders the stored row, not the draft it submitted", () => {
    // If the reducer kept the submitted text, the composer would be displaying a message the database never
    // received. The row is what the service returned.
    const sending = chatReducer(editing("a draft that differs from the stored body"), { type: "submit" });
    const contribution = recordedOf(chatReducer(sending, { type: "recorded", contribution: stored }));
    assert.equal(contribution.body, stored.body);
    assert.equal(contribution.contribution_id, stored.contribution_id);
  });

  test("an unrouted contribution records no epoch change", () => {
    // The record must not imply project truth changed. Nothing routes a contribution to an owning service yet,
    // so the outcome is PENDING and the epoch pair is equal (DEC-030).
    const contribution = recordedOf(
      chatReducer(chatReducer(editing("hello"), { type: "submit" }), {
        type: "recorded",
        contribution: stored,
      }),
    );
    assert.equal(contribution.result_type, "PENDING");
    assert.equal(contribution.result_reference, null);
    assert.equal(contribution.epoch_after, contribution.epoch_before);
  });

  test("the tray survives recording, and editing returns to composing", () => {
    const before = editing("hello", unresolvedTray());
    const recorded = chatReducer(chatReducer(before, { type: "submit" }), {
      type: "recorded",
      contribution: stored,
    });
    assert.equal(chatTray(recorded).entries.length, 1);

    const next = chatReducer(recorded, { type: "edit", text: "and another thing" });
    assert.equal(next.kind, "editing");
    assert.equal(next.text, "and another thing");
    assert.equal(chatTray(next).entries.length, 1);
  });

  test("a submit carrying no text is refused by the reducer, not only by the predicate", () => {
    // The predicate and the reducer must agree. A caller that dispatches submit directly must not be able to
    // produce a contribution carrying no user text.
    const blank = editing("   ");
    assert.equal(chatReducer(blank, { type: "submit" }), blank);
  });
});
