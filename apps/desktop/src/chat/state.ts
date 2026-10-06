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
// The message half is recorded by `record_user_contribution` (DEC-030), which the contract declares and the
// Rust side implements. The recorded state carries the row Rust stored rather than the draft that was
// submitted, so the composer cannot display a message the database never received. `result_type` is `PENDING`
// because nothing routes a contribution to an owning service yet: the record says what happened - the user
// contributed this text and it was labelled for routing - and does not claim project truth changed.

import { trayReducer, type AttachmentAction, type AttachmentTray } from "../attachments/state.ts";
import type { CommandError } from "../intake/state.ts";

/**
 * The advisory label produced for routing.
 *
 * Advisory only. The owning service determines materiality, and this value is never read as authorization.
 */
export type ContributionClassification = "MATERIAL" | "CONTEXT" | "COMMENTARY";

/**
 * What the owning service actually did about a contribution.
 *
 * `PENDING` means recorded with nothing having ruled on it. It is displayed as such rather than rendered as a
 * change, so a contribution the service has not assessed is never shown as having changed project truth.
 */
export type ContributionResult = "EPOCH_ADVANCED" | "CONTEXT_SNAPSHOT" | "NO_CHANGE" | "PENDING";

/**
 * One recorded contribution, as Rust stored it.
 *
 * `classification` is the advisory label and `result_type` is what the service did; the Control Room displays
 * the outcome rather than the label.
 */
export type UserContribution = {
  contribution_id: string;
  project_id: string;
  body: string;
  classification: ContributionClassification;
  classification_confidence: number | null;
  classification_source: "INTAKE_ROUTER";
  result_type: ContributionResult;
  result_reference: string | null;
  epoch_before: number;
  epoch_after: number;
  created_at: string;
};

/**
 * The composer's state.
 *
 * `sending` and `refused` exist because the enablement rule and the failure path are both real. `recorded`
 * exists because `record_user_contribution` is declared and implemented, and a state nothing can produce would
 * be a state machine documenting a feature that does not exist.
 */
export type ChatState =
  /** The user is composing. The text is a local draft; nothing has been recorded. */
  | { kind: "editing"; text: string; tray: AttachmentTray }
  /** The message is in flight. The draft is retained and no authoritative field is shown. */
  | { kind: "sending"; text: string; tray: AttachmentTray }
  /**
   * The contribution is durable. `contribution` is the row Rust stored, not the draft, and the draft is cleared
   * because it is now a record rather than something still being composed.
   */
  | { kind: "recorded"; text: string; tray: AttachmentTray; contribution: UserContribution }
  /** The attempt was refused. The draft and the tray are retained so nothing has to be retyped or re-picked. */
  | { kind: "refused"; text: string; tray: AttachmentTray; error: CommandError };

export type ChatAction =
  | { type: "edit"; text: string }
  /** A tray action, delegated so one reducer owns the attachments and the composer does not re-implement it. */
  | { type: "attachment"; action: AttachmentAction }
  | { type: "submit" }
  /** The service recorded the contribution. Carries the stored row, never the draft. */
  | { type: "recorded"; contribution: UserContribution }
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
      // Editing always returns to composing, including from `recorded`. The recorded contribution stays
      // recorded; the composer is a draft again. Spreading `state` here would carry `contribution` into a
      // state that has no such field, which is how a stale row gets displayed as the current draft.
      return { kind: "editing", text: action.text, tray: state.tray };

    case "attachment":
      // The tray transitions exactly as the shared reducer defines them, for both surfaces. There is no
      // chat-specific attachment rule, which is why there is no second reducer here to drift from it.
      return { ...state, tray: trayReducer(state.tray, action.action) };

    case "submit":
      // Guard a double submit, as intake does, and refuse a submit with no text: the enablement rule and the
      // reducer must agree, or a caller that bypasses the predicate can send an empty contribution.
      if (state.kind === "sending" || state.text.trim() === "") return state;
      return { kind: "sending", text: state.text, tray: state.tray };

    case "recorded":
      // The draft is cleared because it is now durable. Retaining it would let one intent be submitted twice and
      // recorded as two contributions.
      return { kind: "recorded", text: "", tray: state.tray, contribution: action.contribution };

    case "refused":
      return { kind: "refused", text: state.text, tray: state.tray, error: action.error };
  }
}
