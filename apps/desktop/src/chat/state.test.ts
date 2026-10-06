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
    // carries no user text, which is not a UserContribution.
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
