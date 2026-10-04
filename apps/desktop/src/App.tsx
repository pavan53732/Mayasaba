import { useCallback, useMemo, useReducer, useState } from "react";

import { createProject, pickFolder, validateWorkspace } from "./intake/bridge";
import {
  EMPTY_DRAFT,
  initialState,
  intakeReducer,
  isSubmitting,
  emptyWorkspace,
  isAuthorized,
  workspaceReducer,
  type CommandError,
  type Draft,
  type ProjectView,
  type WorkspaceState,
} from "./intake/state";

// Mayasaba Control Room: the Initial Intake Composer.
//
// React owns presentation and draft state. Rust owns project truth. This component never constructs a
// project from the form - after a successful command it renders only the projection the service returned,
// because the accepted state contains no draft to render.
//
// This is the first vertical slice through the UI boundary:
//   composer -> local draft -> create_project (Tauri) -> ProjectService -> SQLite -> authoritative
//   projection -> React renders stored state.

export default function App() {
  const [state, dispatch] = useReducer(intakeReducer, initialState);
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [workspace, dispatchWorkspace] = useReducer(workspaceReducer, emptyWorkspace);

  const pending = isSubmitting(state);
  const created = state.kind === "created" ? state.project : null;
  const error: CommandError | null = state.kind === "rejected" ? state.error : null;
  const authorized = isAuthorized(workspace);

  const onBrowse = useCallback(async () => {
    dispatchWorkspace({ type: "browse" });
    const picked = await pickFolder();
    if (picked === null) {
      dispatchWorkspace({ type: "cancelled" });
      return;
    }
    dispatchWorkspace({ type: "selected", path: picked });
    // Selection is not authorization. Rust decides whether the folder is usable.
    const check = await validateWorkspace(picked);
    if (check.status === "AUTHORIZED" && check.canonicalPath) {
      dispatchWorkspace({ type: "authorized", canonicalPath: check.canonicalPath });
    } else {
      dispatchWorkspace({
        type: "rejected",
        code: check.code ?? "WORKSPACE_NOT_ACCESSIBLE",
        message: check.message ?? "That folder could not be used as a workspace.",
      });
    }
  }, []);

  const onSubmit = useCallback(async () => {
    dispatch({ type: "submit" });
    const result = await createProject({
      name: draft.name,
      local_path: authorized ? workspace.canonicalPath : draft.localPath,
      initial_brief: draft.initialBrief,
    });

    if (isProjectView(result)) {
      // Only the returned projection crosses into state. The draft is not sent along.
      dispatch({ type: "accepted", project: result });
      return;
    }
    dispatch({ type: "rejected", error: result });
  }, [draft, authorized, workspace]);

  // Create requires an authorized workspace. A candidate or an invalid path cannot be submitted, so a
  // string the UI merely holds can never become a project workspace root.
  const canSubmit = useMemo(
    () =>
      !pending &&
      authorized &&
      draft.name.trim() !== "" &&
      draft.initialBrief.trim() !== "",
    [pending, authorized, draft],
  );

  return (
    <main style={{ fontFamily: "system-ui", padding: 32, maxWidth: 760, margin: "0 auto" }}>
      <h1 style={{ marginBottom: 4 }}>Mayasaba</h1>
      <p style={{ color: "#6b7280", marginTop: 0 }}>
        Windows local-first control plane for coordinating CLI coding agents.
      </p>

      {created ? (
        <ProjectPanel project={created} onStartAnother={() => { setDraft(EMPTY_DRAFT); dispatchWorkspace({ type: "edit", requestedPath: "" }); dispatch({ type: "edit", draft: EMPTY_DRAFT }); }} />
      ) : (
        <Composer
          draft={draft}
          onChange={setDraft}
          workspace={workspace}
          onBrowse={onBrowse}
          onEditWorkspace={(requestedPath) => dispatchWorkspace({ type: "edit", requestedPath })}
          pending={pending}
          canSubmit={canSubmit}
          onSubmit={onSubmit}
          error={error}
        />
      )}
    </main>
  );
}

function Composer(props: {
  draft: Draft;
  onChange: (draft: Draft) => void;
  workspace: WorkspaceState;
  onBrowse: () => void;
  onEditWorkspace: (requestedPath: string) => void;
  pending: boolean;
  canSubmit: boolean;
  onSubmit: () => void;
  error: CommandError | null;
}) {
  const { draft, onChange, workspace, onBrowse, onEditWorkspace, pending, canSubmit, onSubmit, error } = props;
  const selecting = workspace.kind === "selecting";
  const authorized = isAuthorized(workspace);

  return (
    <section aria-label="Initial Intake Composer">
      <h2>New project</h2>
      <p style={{ color: "#6b7280", marginTop: -4 }}>
        Choose the local workspace Mayasaba is authorized to work in, then describe what you want
        accomplished.
      </p>

      <label style={field}>
        Project name
        <input
          value={draft.name}
          disabled={pending}
          onChange={(e) => onChange({ ...draft, name: e.target.value })}
          style={input}
        />
      </label>

      <div style={field}>
        <span>Local workspace</span>
        <div style={{ display: "flex", gap: 8, marginTop: 6 }}>
          <input
            value={workspace.kind === "authorized" ? workspace.canonicalPath : workspace.kind === "candidate" || workspace.kind === "invalid" ? workspace.requestedPath : ""}
            disabled={pending || selecting}
            onChange={(e) => onEditWorkspace(e.target.value)}
            style={{ ...input, marginTop: 0 }}
          />
          <button type="button" onClick={onBrowse} disabled={pending || selecting} style={browseButton}>
            {selecting ? "Opening…" : "Browse…"}
          </button>
        </div>
        <WorkspaceStatus workspace={workspace} />
      </div>

      <label style={field}>
        Describe the project
        <textarea
          value={draft.initialBrief}
          disabled={pending}
          rows={6}
          onChange={(e) => onChange({ ...draft, initialBrief: e.target.value })}
          style={{ ...input, height: "auto", resize: "vertical" }}
        />
      </label>
      <p style={{ ...hint, marginTop: -8, marginBottom: 14 }}>
        This becomes <strong>ProjectBrief version 1</strong> — the canonical record of your project intent.
        It stays a local draft until the project is created.
      </p>

      <button onClick={onSubmit} disabled={!canSubmit} style={button}>
        {pending ? "Creating…" : "Create project"}
      </button>
      {!authorized ? (
        <p style={hint}>Select a local folder to continue. A path must be verified before it can be a workspace.</p>
      ) : null}

      {pending ? (
        <p role="status" style={notice}>
          Submitting. Nothing is stored yet.
        </p>
      ) : null}

      {error ? (
        <div role="alert" style={{ ...notice, borderColor: "#b91c1c", color: "#b91c1c" }}>
          <strong>{error.code}</strong>
          <div style={{ marginTop: 4 }}>{error.message}</div>
          <div style={{ marginTop: 6, color: "#6b7280" }}>Your draft has been kept.</div>
        </div>
      ) : null}
    </section>
  );
}

/** Shows what Rust decided about the workspace. It never infers authorization from the text in the field. */
function WorkspaceStatus({ workspace }: { workspace: WorkspaceState }) {
  switch (workspace.kind) {
    case "authorized":
      return (
        <p style={{ ...hint, marginTop: 6, marginBottom: 0, color: "#166534" }}>
          Local folder verified: <code>{workspace.canonicalPath}</code>
        </p>
      );
    case "candidate":
      return (
        <p style={{ ...hint, marginTop: 6, marginBottom: 0 }}>
          Checking this folder… it is not a workspace until Mayasaba verifies it.
        </p>
      );
    case "invalid":
      return (
        <p role="alert" style={{ ...hint, marginTop: 6, marginBottom: 0, color: "#b91c1c" }}>
          <strong>{workspace.code}</strong> — {workspace.message}
        </p>
      );
    case "selecting":
      return <p style={{ ...hint, marginTop: 6, marginBottom: 0 }}>Choose a folder…</p>;
    case "empty":
      return (
        <p style={{ ...hint, marginTop: 6, marginBottom: 0 }}>
          A folder Mayasaba may operate within. Nothing is authorized until it is verified.
        </p>
      );
  }
}

/**
 * Renders stored state only.
 *
 * Every value below comes from the ProjectView the service returned. Nothing here is derived from the
 * submitted form, which is the architectural point the slice exists to demonstrate.
 */
function ProjectPanel(props: { project: ProjectView; onStartAnother: () => void }) {
  const { project, onStartAnother } = props;
  return (
    <section aria-label="Project" style={{ border: "1px solid #e5e7eb", borderRadius: 8, padding: 20 }}>
      <p style={{ margin: 0, color: "#6b7280", fontSize: 13 }}>Persisted project state</p>
      <h2 style={{ margin: "4px 0 16px" }}>{project.name}</h2>

      <dl style={{ display: "grid", gridTemplateColumns: "160px 1fr", gap: "8px 12px", margin: 0 }}>
        <Term>Project ID</Term><Value>{project.projectId}</Value>
        <Term>Workspace</Term><Value>{project.localPath}</Value>
        <Term>Phase</Term><Value>{project.phase}</Value>
        <Term>Status</Term><Value>{project.status}</Value>
        <Term>Epoch</Term><Value>{project.currentEpoch}</Value>
        <Term>Brief ID</Term><Value>{project.briefId ?? "—"}</Value>
        <Term>Brief version</Term><Value>{project.briefVersion ?? "—"}</Value>
      </dl>

      <h3 style={{ marginBottom: 4 }}>Project brief v{project.briefVersion ?? 1}</h3>
      <p style={{ whiteSpace: "pre-wrap", marginTop: 0 }}>{project.briefBody ?? "—"}</p>

      <button onClick={onStartAnother} style={{ ...button, marginTop: 8, background: "#fff", color: "#111827" }}>
        New project
      </button>
    </section>
  );
}

const Term = ({ children }: { children: React.ReactNode }) => (
  <dt style={{ color: "#6b7280" }}>{children}</dt>
);
const Value = ({ children }: { children: React.ReactNode }) => (
  <dd style={{ margin: 0, fontFamily: "ui-monospace, monospace", fontSize: 13 }}>{children}</dd>
);

function isProjectView(value: ProjectView | CommandError): value is ProjectView {
  return typeof (value as ProjectView).projectId === "string";
}

const field: React.CSSProperties = { display: "block", marginBottom: 14, fontSize: 14, fontWeight: 500 };
const input: React.CSSProperties = {
  display: "block",
  marginTop: 6,
  padding: "8px 10px",
  width: "100%",
  border: "1px solid #d1d5db",
  borderRadius: 6,
  fontWeight: 400,
  fontSize: 14,
  fontFamily: "inherit",
  boxSizing: "border-box",
};
const button: React.CSSProperties = {
  padding: "9px 16px",
  borderRadius: 6,
  border: "1px solid #111827",
  background: "#111827",
  color: "#fff",
  fontSize: 14,
  cursor: "pointer",
};
const notice: React.CSSProperties = {
  marginTop: 14,
  padding: "10px 12px",
  border: "1px solid #e5e7eb",
  borderRadius: 6,
  fontSize: 13,
};
const hint: React.CSSProperties = { color: "#6b7280", fontSize: 12, lineHeight: 1.5 };
const browseButton: React.CSSProperties = {
  padding: "8px 14px",
  borderRadius: 6,
  border: "1px solid #d1d5db",
  background: "#fff",
  color: "#111827",
  fontSize: 14,
  cursor: "pointer",
  whiteSpace: "nowrap",
};