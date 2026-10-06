// Ongoing Chat Composer state.
//
// Available after project creation (INTERNAL-APPLICATION-ARCHITECTURE.md:309). Free-text input becomes a
// `UserContribution` with an advisory classification; the owning service decides whether project truth
// changes, and a contribution never mutates authoritative state on its own.
//
// DEC-106 states the optionality rule this module exists to make true: "The chat composer must support
// attachments, and an attachment is never a prerequisite for submitting a normal user message." That is a
// property of the send predicate, so the predicate is a pure function and the property is a test.
//
// WHAT IS NOT HERE, AND WHY. There is no accepted state and no dispatch, because no operation records a
// `UserContribution`. DEC-030 records the requirement - "A new Tauri command is required for classified
// free-text input" - and `schemas/tauri-bridge-v1/payloads.json` declares no such command. Adding one here
// would be inventing an API (AGENTS.md section 8), and a `sent` state nothing can produce would be a state
// machine documenting a feature that does not exist. The attachment half of this surface is wired to the
// three operations the contract does declare; the message half is presentation state plus an enablement rule
// that is already true and already tested. The gap is recorded in docs/ROADMAP.md.

import { trayReducer, type AttachmentAction, type AttachmentTray } from "../attachments/state.ts";
import type { CommandError } from "../intake/state.ts";

/**
 * The composer's state.
 *
 * `sending` and `refused` exist because the enablement rule and the failure path are both real today. There is
 * no `sent`, for the reason given at the top of this file.
 */
export type ChatState =
  /** The user is composing. The text is a local draft; nothing has been recorded. */
  | { kind: "editing"; text: string; tray: AttachmentTray }
  /** The message is in flight. The draft is retained and no authoritative field is shown. */
  | { kind: "sending"; text: string; tray: AttachmentTray }
  /** The attempt was refused. The draft and the tray are retained so nothing has to be retyped or re-picked. */
  | { kind: "refused"; text: string; tray: AttachmentTray; error: CommandError };

export type ChatAction =
  | { type: "edit"; text: string }
  /** A tray action, delegated so one reducer owns the attachments and the composer does not re-implement it. */
  | { type: "attachment"; action: AttachmentAction }
  | { type: "submit" }
  | { type: "refused"; error: CommandError };

export function initialChatState(provenance: "INITIAL_INTAKE_COMPOSER" | "CHAT_COMPOSER"): ChatState {
  return { kind: "editing", text: "", tray: { provenance, entries: [] } };
}

/** The composer's tray, whichever state it is in. */
export function chatTray(state: ChatState): AttachmentTray {
  return state.tray;
}

export function isSending(state: ChatState): boolean {
  return state.kind === "sending";
}

/**
 * Whether the composer may submit the message.
 *
 * The text is the only input, and this function takes no tray at all. That is the strongest available form of
 * "an attachment is never a prerequisite": there is no argument through which an attachment could change the
 * answer, so no future edit to a tray rule can start gating the message. DEC-106 states the rule; the
 * signature is what enforces it.
 *
 * The same reasoning applies one level up: this is not `canSend && attachmentsAreResolved`, and it never
 * becomes that.
 */
export function canSendMessage(state: ChatState): boolean {
  return !isSending(state) && state.text.trim() !== "";
}

export function chatReducer(state: ChatState, action: ChatAction): ChatState {
  switch (action.type) {
    case "edit":
      return { ...state, text: action.text };

    case "attachment":
      // The tray transitions exactly as the shared reducer defines them, for both surfaces. There is no
      // chat-specific attachment rule, which is why there is no second reducer here to drift from it.
      return { ...state, tray: trayReducer(state.tray, action.action) };

    case "submit":
      // Guard a double submit, as intake does. Nothing here claims the message was recorded.
      if (state.kind === "sending") return state;
      return { kind: "sending", text: state.text, tray: state.tray };

    case "refused":
      return { kind: "refused", text: state.text, tray: state.tray, error: action.error };
  }
}
