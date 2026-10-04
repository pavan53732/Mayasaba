// The single controlled transport boundary between the Control Room and Rust.
//
// The generated bridge surface is identifier-level: it gives COMMANDS, QUERIES and their owners, not typed
// request and response models. Scattering raw `invoke("create_project", {...})` through components would
// hand every call site its own idea of the contract, which is the drift this repository keeps removing at
// other layers. So there is exactly one typed wrapper here, and components call it.
//
// The transport is injectable so tests can drive the UI without a Tauri runtime. The default resolves
// `@tauri-apps/api` lazily, so importing this module in a plain Node test does not require Tauri.

import type { CommandError, ProjectView, RecoveryReport } from "./state";

export type Transport = (command: string, args: Record<string, unknown>) => Promise<unknown>;

const tauriTransport: Transport = async (command, args) => {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke(command, args);
};

let transport: Transport = tauriTransport;

/** Replace the transport. Tests only. */
export function setTransport(next: Transport): void {
  transport = next;
}

export function resetTransport(): void {
  transport = tauriTransport;
}

/** The request shape declared by create_projectRequest. Whitespace is preserved on the way out. */
/**
 * List every persisted project, newest first.
 *
 * This is the rehydration path: on launch the Control Room renders what Rust returns rather than anything it
 * retained from a previous session.
 */
export async function listProjects(): Promise<ProjectView[] | CommandError> {
  try {
    const result = await transport("list_projects", {});
    return result as ProjectView[];
  } catch (thrown) {
    const candidate = thrown as Partial<CommandError>;
    if (typeof candidate?.code === "string" && typeof candidate?.message === "string") {
      return candidate as CommandError;
    }
    return { code: "TRANSPORT_FAILURE", message: thrown instanceof Error ? thrown.message : String(thrown) };
  }
}

/** The startup recovery scan result. Recovery reports; it never repairs. */
export async function getRecoveryStatus(): Promise<RecoveryReport | CommandError> {
  try {
    return (await transport("get_recovery_status", {})) as RecoveryReport;
  } catch (thrown) {
    const candidate = thrown as Partial<CommandError>;
    if (typeof candidate?.code === "string" && typeof candidate?.message === "string") {
      return candidate as CommandError;
    }
    return { code: "TRANSPORT_FAILURE", message: thrown instanceof Error ? thrown.message : String(thrown) };
  }
}

/**
 * The request shape declared by `create_projectRequest`. Whitespace is preserved on the way out.
 *
 * A `type` rather than an `interface` for one concrete reason: a type alias gets an implicit index signature,
 * so it is directly assignable to the transport's `Record<string, unknown>`. An interface is not, and the
 * workaround would be a cast at the one call site this file exists to keep honest.
 */
export type CreateProjectRequest = {
  local_path: string;
  initial_brief: string;
};

/** Mirrors WorkspaceCheck in the Tauri command. Wire names, snake_case (DEC-054). */
export interface WorkspaceCheck {
  status: "AUTHORIZED" | "INVALID";
  canonical_path: string | null;
  requested_path: string;
  code: string | null;
  message: string | null;
  /** Derived by Rust from the canonical folder leaf (DEC-050). The UI displays it; it never computes it. */
  derived_project_name: string | null;
}

/**
 * Validate a candidate workspace folder in Rust.
 *
 * The UI must not decide whether a path is a usable workspace. It sends the candidate and renders whatever
 * Rust answers, which is why this returns a check result rather than a boolean the UI interprets.
 */
export async function validateWorkspace(path: string): Promise<WorkspaceCheck> {
  const result = await transport("validate_workspace", { path });
  return result as WorkspaceCheck;
}

/**
 * Open the native Windows folder picker.
 *
 * Returns null when the user cancels. The dialog plugin is resolved lazily so importing this module outside a
 * Tauri runtime does not fail.
 */
export async function pickFolder(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({ directory: true, multiple: false, title: "Select the local workspace for this project" });
  return typeof selected === "string" ? selected : null;
}

/**
 * Create a project and return the authoritative projection the service committed.
 *
 * Rejections are returned, not thrown: a refused command is an expected outcome of intake, and the UI needs
 * the code to render a message without inspecting prose.
 */
export async function createProject(
  request: CreateProjectRequest,
): Promise<ProjectView | CommandError> {
  try {
    // The request is already the wire shape, field for field, so it is sent as it stands. There is no name
    // translation here any more: the translation this replaced (`local_path` -> `localPath`) was where a second
    // spelling of every field was introduced, and keeping it in step with Rust by hand is what failed (DEC-054).
    const result = await transport("create_project", request);
    return result as ProjectView;
  } catch (thrown) {
    const candidate = thrown as Partial<CommandError>;
    if (typeof candidate?.code === "string" && typeof candidate?.message === "string") {
      return candidate as CommandError;
    }
    // A rejection that is not already a CommandError would be a contract violation on the Rust side. Surface
    // it as one rather than inventing an optimistic success, which is how a UI ends up displaying authority
    // it never received.
    return {
      code: "TRANSPORT_FAILURE",
      message: thrown instanceof Error ? thrown.message : String(thrown),
    };
  }
}