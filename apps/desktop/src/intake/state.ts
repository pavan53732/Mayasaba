// Intake state: the authority boundary, expressed as a pure reducer.
//
// This file deliberately has no React import. The rule that matters - after a successful command the UI
// displays what the service committed, never the submitted draft - is a state transition, so it is written
// as one and tested without a DOM. A component that renders `state` cannot violate the rule, because the
// draft is not present in the accepted state at all.
//
// That is the difference between documenting the rule and enforcing it.

// Mirrors ProjectView in apps/desktop/src-tauri/src/main.rs. Rust owns this shape; the UI consumes it.
export interface ProjectView {
  projectId: string;
  name: string;
  localPath: string;
  phase: string;
  status: string;
  currentEpoch: number;
  briefId: string | null;
  briefVersion: number | null;
  briefBody: string | null;
  createdAt: string;
}

// Mirrors CommandError in the Tauri command. A machine-readable code plus a human message.
export interface CommandError {
  code: string;
  message: string;
}

/**
 * What the user actually supplies at intake.
 *
 * There is deliberately no `name`: the display name is derived by Rust from the authorized workspace folder
 * (DEC-050), so the draft must not contain a second, competing name. The draft is only user input.
 */
export interface Draft {
  localPath: string;
  initialBrief: string;
}

export const EMPTY_DRAFT: Draft = { localPath: "", initialBrief: "" };

/**
 * Workspace selection state.
 *
 * Selection is not authorization. `candidate` means the user picked or typed something that has not been
 * checked; only `authorized` came back from Rust having verified the folder exists, is local, and is a
 * directory. Create requires `authorized`, so an unverified string can never become a project workspace root.
 *
 * The same reasoning as the project-truth boundary applies one level down: the UI may represent a candidate,
 * but only Rust establishes the workspace.
 */
/**
 * A picker is open. `retain` is the state cancelling restores, so an abandoned browse cannot discard a
 * workspace the user already chose. Declared as an interface because a type alias cannot reference itself.
 */
export interface SelectingWorkspace {
  kind: "selecting";
  retain: WorkspaceState;
}

export interface CandidateWorkspace {
  kind: "candidate";
  requestedPath: string;
}

export interface InvalidWorkspace {
  kind: "invalid";
  requestedPath: string;
  code: string;
  message: string;
}

export interface AuthorizedWorkspace {
  kind: "authorized";
  requestedPath: string;
  canonicalPath: string;
  /** Derived by Rust from the canonical folder leaf. The UI displays it; it never computes it. */
  derivedProjectName: string;
}

export type WorkspaceState =
  | { kind: "empty" }
  | SelectingWorkspace
  | CandidateWorkspace
  | InvalidWorkspace
  | AuthorizedWorkspace;

export type WorkspaceAction =
  | { type: "browse" }
  | { type: "selected"; path: string }
  | { type: "cancelled" }
  | { type: "checking" }
  | { type: "authorized"; canonicalPath: string; derivedProjectName: string }
  | { type: "rejected"; code: string; message: string }
  | { type: "edit"; requestedPath: string };

export const emptyWorkspace: WorkspaceState = { kind: "empty" };

export function workspaceReducer(state: WorkspaceState, action: WorkspaceAction): WorkspaceState {
  switch (action.type) {
    case "browse":
      // Guard a second dialog while one is already open.
      if (state.kind === "selecting") return state;
      return { kind: "selecting", retain: state };

    case "selected":
      // A selection is only a candidate until Rust validates it.
      return { kind: "candidate", requestedPath: action.path };

    case "cancelled":
      // Cancelling the picker changes nothing: the state the browse started from is restored exactly.
      if (state.kind !== "selecting") return state;
      return state.retain;

    case "checking":
      if (state.kind !== "candidate") return state;
      return state;

    case "authorized":
      return {
        kind: "authorized",
        requestedPath: state.kind === "candidate" ? state.requestedPath : action.canonicalPath,
        canonicalPath: action.canonicalPath,
        derivedProjectName: action.derivedProjectName,
      };

    case "rejected":
      return {
        kind: "invalid",
        requestedPath: state.kind === "candidate" ? state.requestedPath : "",
        code: action.code,
        message: action.message,
      };

    case "edit":
      // Manual entry produces a candidate, never an authorization.
      return action.requestedPath.trim() === ""
        ? emptyWorkspace
        : { kind: "candidate", requestedPath: action.requestedPath };
  }
}

/** Only an authorized workspace may be submitted. */
export function isAuthorized(
  state: WorkspaceState,
): state is { kind: "authorized"; requestedPath: string; canonicalPath: string; derivedProjectName: string } {
  return state.kind === "authorized";
}

/** The startup recovery scan result. Recovery reports; it never repairs. */
export interface RecoveryIssue {
  kind: string;
  detail: string;
}

export interface RecoveryReport {
  clean: boolean;
  integrityOk: boolean;
  issues: RecoveryIssue[];
}

export type IntakeState =
  /** The composer holds unpersisted input. Nothing authoritative exists yet. */
  | { kind: "editing"; draft: Draft }
  /** The command is in flight. The draft is retained; no authoritative field may be shown. */
  | { kind: "submitting"; draft: Draft }
  /** The service committed. Only its projection exists here. */
  | { kind: "created"; project: ProjectView }
  /** The command was refused. The draft is retained so the user does not retype it. */
  | { kind: "rejected"; draft: Draft; error: CommandError };

export type IntakeAction =
  | { type: "edit"; draft: Draft }
  | { type: "submit" }
  | { type: "accepted"; project: ProjectView }
  | { type: "rejected"; error: CommandError };

export const initialState: IntakeState = { kind: "editing", draft: EMPTY_DRAFT };

/** The draft, when the state has one. An accepted state intentionally has none. */
export function draftOf(state: IntakeState): Draft {
  switch (state.kind) {
    case "editing":
    case "submitting":
    case "rejected":
      return state.draft;
    case "created":
      // There is no draft after acceptance. Returning EMPTY_DRAFT rather than throwing keeps callers that
      // want to restore the composer simple, and `hasDraft` is available when the distinction matters.
      return EMPTY_DRAFT;
  }
}

/** True when the UI is holding unsubmitted input. False once the service has committed. */
export function hasDraft(state: IntakeState): boolean {
  return state.kind !== "created";
}

export function isSubmitting(state: IntakeState): boolean {
  return state.kind === "submitting";
}

export function intakeReducer(state: IntakeState, action: IntakeAction): IntakeState {
  switch (action.type) {
    case "edit":
      return { kind: "editing", draft: action.draft };

    case "submit":
      // Guard a double submit. The command is not idempotent (DEC-047), so a second in-flight request could
      // create a second project. Disabling the button is the UI half; this is the state half.
      if (state.kind === "submitting") return state;
      if (state.kind === "created") return state;
      return { kind: "submitting", draft: state.draft };

    case "accepted":
      // THE AUTHORITY BOUNDARY. The new state is constructed from the service's projection alone. No draft
      // field is copied, so the UI cannot display submitted input after success even if it wanted to: the
      // submitted string simply is not in this state. The durable test is in state.test.ts, which commits a
      // project whose fields differ from the submitted draft and asserts the stored values win.
      return { kind: "created", project: action.project };

    case "rejected":
      return { kind: "rejected", draft: draftOf(state), error: action.error };
  }
}