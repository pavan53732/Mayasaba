import { useCallback, useMemo, useReducer, useState } from "react";

import { createProject } from "./intake/bridge";
import {
  EMPTY_DRAFT,
  initialState,
  intakeReducer,
  isSubmitting,
  type CommandError,
  type Draft,
  type ProjectView,
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

  const pending = isSubmitting(state);
  const created = state.kind === "created" ? state.project : null;
  const error: CommandError | null = state.kind === "rejected" ? state.error : null;

  const onSubmit = useCallback(async () => {
    dispatch({ type: "submit" });
    const result = await createProject({
      name: draft.name,
      local_path: draft.localPath,
      initial_brief: draft.initialBrief,
    });

    if (isProjectView(result)) {
      // Only the returned projection crosses into state. The draft is not sent along.
      dispatch({ type: "accepted", project: result });
      return;
    }
    dispatch({ type: "rejected", error: result });
  }, [draft]);

  const canSubmit = useMemo(
    () => !pending && draft.name.trim() !== "" && draft.localPath.trim() !== "" && draft.initialBrief.trim() !== "",
    [pending, draft],
  );

  return (
    <main style={{ fontFamily: "system-ui", padding: 32, maxWidth: 760, margin: "0 auto" }}>
      <h1 style={{ marginBottom: 4 }}>Mayasaba</h1>
      <p style={{ color: "#6b7280", marginTop: 0 }}>
        Windows local-first control plane for coordinating CLI coding agents.
      </p>

      {created ? (
        <ProjectPanel project={created} onStartAnother={() => { setDraft(EMPTY_DRAFT); dispatch({ type: "edit", draft: EMPTY_DRAFT }); }} />
      ) : (
        <Composer
          draft={draft}
          onChange={setDraft}
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
  pending: boolean;
  canSubmit: boolean;
  onSubmit: () => void;
  error: CommandError | null;
}) {
  const { draft, onChange, pending, canSubmit, onSubmit, error } = props;

  return (
    <section aria-label="Initial Intake Composer">
      <h2>New project</h2>
      <p style={{ color: "#6b7280", marginTop: -4 }}>
        State what you want built and select the local workspace. This becomes the first ProjectBrief version.
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

      <label style={field}>
        Local workspace
        <input
          value={draft.localPath}
          disabled={pending}
          placeholder="C:\\work\\my-project"
          onChange={(e) => onChange({ ...draft, localPath: e.target.value })}
          style={input}
        />
      </label>

      <label style={field}>
        What do you want built?
        <textarea
          value={draft.initialBrief}
          disabled={pending}
          rows={5}
          onChange={(e) => onChange({ ...draft, initialBrief: e.target.value })}
          style={{ ...input, height: "auto", resize: "vertical" }}
        />
      </label>

      <button onClick={onSubmit} disabled={!canSubmit} style={button}>
        {pending ? "Creating…" : "Create project"}
      </button>

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