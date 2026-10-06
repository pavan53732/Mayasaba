// Attachment presentation state, shared by both composers.
//
// DEC-106 defines three non-collapsible operations - attach, capture, consume - and makes attachment an
// optional capability on two surfaces: the Initial Intake Composer and the Ongoing Chat Composer. This module
// is the presentation half of that. It holds what the user has selected and what Rust has actually recorded,
// and it holds nothing else.
//
// The rule it enforces is the one CONTROL-ROOM-DESIGN.md states: the UI never renders an attachment as
// uploaded, indexed, analysed or accepted before the controller returns that state. So there is no local
// "attached" flag and no local lifecycle. An entry becomes durable when a resolution comes back from Rust, and
// the lifecycle, the resolvability verdict and the capture and consume facts are all read off that answer. A
// component rendering this state cannot claim more than Rust said, because the state does not contain more.
//
// Like intake/state.ts, this file has no React import. The rules are state transitions, so they are written as
// a reducer and tested without a DOM. That is the difference between documenting the rule and enforcing it.

/** Which surface offered a reference. The two values the contract declares (DEC-106). */
export type AttachmentProvenance = "INITIAL_INTAKE_COMPOSER" | "CHAT_COMPOSER";

/**
 * The recorded lifecycle of an attachment, owned by `AttachmentService` (DEC-107).
 *
 * This is deliberately not the intake submission vocabulary (`editing`/`submitting`/`created`/`rejected`) and
 * not the workspace vocabulary (`candidate`/`authorized`). One word for two concepts is how a UI ends up
 * rendering a draft as though it were durable, which is the confusion this vocabulary exists to prevent.
 */
export type AttachmentLifecycle = "SELECTED" | "PENDING" | "ACCEPTED" | "REJECTED";

/** The checks a resolvability evaluation performs, in the order it performs them. */
export type AttachmentCheckName = "EXISTS" | "LOCALITY" | "KIND" | "SCOPE";

/**
 * The Admission check status vocabulary, reused rather than re-invented (DEC-106).
 *
 * `NOT_APPLICABLE` means the check was not reached because an earlier one already decided the verdict. It is
 * never reported as `PASS`, because a check that was not performed did not pass.
 */
export type CheckStatus = "PASS" | "FAIL" | "NOT_APPLICABLE";

/**
 * The one verdict of a resolvability evaluation.
 *
 * Computed when the row is read and never stored, so it is an observation about the reference rather than a
 * property of the attachment: moving a file makes the same row `UNRESOLVED` without the row changing.
 */
export type ResolvabilityVerdict = "RESOLVED" | "UNRESOLVED";

/**
 * Mirrors `AttachmentView` in the Tauri command. Wire names, snake_case (DEC-054).
 *
 * `content_hash` and `context_evidence_id` are nullable rather than optional, and they arrive as `null`
 * rather than being absent, so "not captured yet" is distinguishable from "this build does not report
 * hashes".
 */
export interface AttachmentView {
  attachment_id: string;
  project_id: string;
  /** Rust's canonical path, recorded at attach time and never rewritten. */
  source_path: string;
  kind: "FILE" | "DIRECTORY";
  authorized_scope: string;
  provenance: AttachmentProvenance;
  lifecycle_state: AttachmentLifecycle;
  /** Set only by an explicit capture. `null` is "not captured", never "unknown". */
  content_hash: string | null;
  /** Set only by an explicit consume. `null` is "no owning service accepted a change from this". */
  context_evidence_id: string | null;
  captured_at: string;
}

/** Mirrors `AttachmentCheckView`. */
export interface AttachmentCheck {
  check: AttachmentCheckName;
  status: CheckStatus;
  code: string | null;
  detail: string | null;
}

/** Mirrors `AttachmentResolutionView`: the stored row plus the evaluation performed when it was read. */
export interface AttachmentResolution {
  attachment: AttachmentView;
  verdict: ResolvabilityVerdict;
  checks: AttachmentCheck[];
}

/**
 * One entry in an attachment tray.
 *
 * `candidate`, `attaching` and `refused` are interaction state: the UI knows a path and nothing more. Only
 * `attached` carries a resolution, and that resolution is Rust's. The four cases are separate rather than one
 * record with flags, so a component cannot read a resolution out of an entry that has none.
 */
export type AttachmentEntry =
  /** The user selected this path. Nothing has been recorded and nothing has been read. */
  | { kind: "candidate"; requestedPath: string }
  /** The attach command is in flight. Still nothing recorded. */
  | { kind: "attaching"; requestedPath: string }
  /** Rust recorded the reference. `resolution` is its answer, not the UI's expectation. */
  | { kind: "attached"; resolution: AttachmentResolution }
  /** Rust refused. The user's spelling is kept so the message has something to point at. */
  | { kind: "refused"; requestedPath: string; code: string; message: string };

/**
 * The attachments a surface is holding.
 *
 * `provenance` is per tray rather than per entry because one composer has exactly one provenance, and a call
 * site that could name it per entry could also name it wrongly.
 */
export interface AttachmentTray {
  provenance: AttachmentProvenance;
  entries: AttachmentEntry[];
}

export function emptyTray(provenance: AttachmentProvenance): AttachmentTray {
  return { provenance, entries: [] };
}

export type AttachmentAction =
  /** The user picked or typed a path. Not recorded, not read, not verified. */
  | { type: "selected"; path: string }
  | { type: "attaching"; requestedPath: string }
  /**
   * Rust returned a stored row. `requestedPath` is the correlation key only: it names which in-flight entry
   * this answers. It is deliberately not used as the displayed path, because the user's spelling and the
   * stored canonical path are different strings and only one of them is durable.
   */
  | { type: "attached"; requestedPath: string; resolution: AttachmentResolution }
  | { type: "refused"; requestedPath: string; code: string; message: string }
  /** Replace the durable portion with what Rust currently stores. The rehydration path. */
  | { type: "rehydrated"; resolutions: AttachmentResolution[] }
  /** Discard an unrecorded selection. */
  | { type: "discarded"; requestedPath: string }
  /**
   * Start an empty tray for a new surface instance.
   *
   * This is not a way to remove a stored reference. It exists because a tray describes the context of **one**
   * project, and the intake composer begins a new project: without this, the composer for the second project
   * would render the first project's references, which is worse than showing none. Nothing is deleted - the
   * rows stay stored and the project surfaces read them back from Rust - so a cleared tray is a change of
   * subject, not a deletion.
   */
  | { type: "cleared" };

/**
 * The entries that have been sent to Rust and have not answered yet.
 *
 * Named as its own type so the reducer can narrow to it: correlation by `requestedPath` is only meaningful for
 * an entry that is still waiting, and `attached` deliberately has no `requestedPath` to correlate on.
 */
type InFlightEntry = Extract<AttachmentEntry, { kind: "candidate" | "attaching" }>;

/** True while an entry has been sent to Rust and has not answered yet. */
function isInFlight(entry: AttachmentEntry): entry is InFlightEntry {
  return entry.kind === "candidate" || entry.kind === "attaching";
}

export function trayReducer(tray: AttachmentTray, action: AttachmentAction): AttachmentTray {
  switch (action.type) {
    case "selected":
      // A selection is a candidate, exactly as a chosen workspace folder is. Nothing has been recorded, and
      // the UI must not render it as though something had been.
      return { ...tray, entries: [...tray.entries, { kind: "candidate", requestedPath: action.path }] };

    case "attaching":
      return {
        ...tray,
        entries: tray.entries.map((entry) =>
          isInFlight(entry) && entry.requestedPath === action.requestedPath
            ? { kind: "attaching", requestedPath: action.requestedPath }
            : entry,
        ),
      };

    case "attached":
      // THE AUTHORITY BOUNDARY. The entry is rebuilt from the resolution alone. The path the UI held is not
      // carried into it, so the displayed path is Rust's canonical path and a component cannot render a
      // spelling the service never recorded.
      return {
        ...tray,
        entries: tray.entries.map((entry) =>
          isInFlight(entry) && entry.requestedPath === action.requestedPath
            ? { kind: "attached", resolution: action.resolution }
            : entry,
        ),
      };

    case "refused":
      // A refusal keeps the entry and the user's spelling, so the reason has something to point at and the
      // selection is not silently lost.
      return {
        ...tray,
        entries: tray.entries.map((entry) =>
          isInFlight(entry) && entry.requestedPath === action.requestedPath
            ? {
                kind: "refused",
                requestedPath: action.requestedPath,
                code: action.code,
                message: action.message,
              }
            : entry,
        ),
      };

    case "rehydrated":
      // Durable entries come from Rust and are listed first, oldest first, in the order the service returned
      // them. Entries the user has picked but that are not recorded are interaction state and survive, because
      // they were never part of what is being replaced.
      return {
        ...tray,
        entries: [
          ...action.resolutions.map(
            (resolution): AttachmentEntry => ({ kind: "attached", resolution }),
          ),
          ...tray.entries.filter((entry) => !isDurable(entry)),
        ],
      };

    case "discarded":
      // Only an unrecorded selection can be discarded. A stored reference is durable and is never deleted
      // (DEC-106), so there is deliberately no action that removes an attached entry: dropping one from the
      // tray would misrepresent stored state as absent. Detaching an association is a different operation, and
      // no contract declares one yet.
      return {
        ...tray,
        entries: tray.entries.filter(
          (entry) => !(entry.kind === "candidate" && entry.requestedPath === action.requestedPath),
        ),
      };

    case "cleared":
      // A change of subject, not a deletion: the rows stay stored and are re-read from Rust by whichever
      // surface is about that project. See the action's own comment.
      return { ...tray, entries: [] };
  }
}

/** True when Rust has recorded this entry. */
export function isDurable(entry: AttachmentEntry): boolean {
  return entry.kind === "attached";
}

/**
 * The entries Rust has recorded.
 *
 * The only entries that may be described as attached. A caller that wants "how many attachments does this
 * surface have" wants this, not `entries.length`.
 */
export function durableEntries(tray: AttachmentTray): AttachmentEntry[] {
  return tray.entries.filter(isDurable);
}

/**
 * The paths this surface has queued and not yet recorded.
 *
 * The intake path needs this because an attachment is project-scoped and the project does not exist while the
 * user is still composing: there is no `project_id` to attach to until `create_project` returns one. The
 * selections are therefore held as candidates across the creation command, and this is the list that is
 * attached afterwards, in order.
 */
export function queuedPaths(tray: AttachmentTray): string[] {
  return tray.entries
    .filter((entry) => entry.kind === "candidate")
    .map((entry) => entry.requestedPath);
}

/**
 * What the UI may say about one entry.
 *
 * Every field is either an interaction fact (the UI knows this path) or a value taken from Rust's answer.
 * Nothing is inferred from anything else, which is what keeps "selected" from being rendered as "attached",
 * and "not captured" from being rendered as "indexed".
 */
export interface AttachmentPresentation {
  /** True only when Rust returned a stored row for this entry. */
  durable: boolean;
  /** The recorded lifecycle, or `null` while nothing is stored. Never computed locally. */
  recordedState: AttachmentLifecycle | null;
  /** The evaluation performed when the row was read, or `null` while nothing is stored. */
  verdict: ResolvabilityVerdict | null;
  checks: AttachmentCheck[];
  /** True only when the stored row carries a content hash, which only an explicit capture sets. */
  captured: boolean;
  /** True only when the stored row carries an evidence link, which only an explicit consume sets. */
  consumed: boolean;
  /** Which composer recorded this reference, or `null` while nothing is stored. */
  provenance: AttachmentProvenance | null;
  /** Rust's canonical path once stored, otherwise the user's spelling. */
  path: string;
  /** A short label for the entry's own state. */
  label: string;
}

export function presentationOf(entry: AttachmentEntry): AttachmentPresentation {
  const unrecorded = (
    path: string,
    label: string,
  ): AttachmentPresentation => ({
    durable: false,
    recordedState: null,
    verdict: null,
    checks: [],
    captured: false,
    consumed: false,
    provenance: null,
    path,
    label,
  });

  switch (entry.kind) {
    case "candidate":
      return unrecorded(entry.requestedPath, "Not attached yet");
    case "attaching":
      return unrecorded(entry.requestedPath, "Attaching…");
    case "refused":
      return unrecorded(entry.requestedPath, `Refused (${entry.code})`);
    case "attached": {
      const { attachment, verdict, checks } = entry.resolution;
      return {
        durable: true,
        recordedState: attachment.lifecycle_state,
        verdict,
        checks,
        captured: attachment.content_hash !== null,
        consumed: attachment.context_evidence_id !== null,
        provenance: attachment.provenance,
        path: attachment.source_path,
        label:
          verdict === "RESOLVED" ? "Attached" : "Attached — the source no longer resolves",
      };
    }
  }
}
