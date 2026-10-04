import { useCallback, useEffect, useMemo, useReducer, useState } from "react";

import { createProject, listProjects, pickFolder, recoveryStatus, validateWorkspace } from "./intake/bridge";
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
  type RecoveryReport,
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
  const [projects, setProjects] = useState<ProjectView[]>([]);
  const [loading, setLoading] = useState(true);
  const [recovery, setRecovery] = useState<RecoveryReport | null>(null);

  // Rehydrate on launch. Everything rendered for an existing project comes from Rust, so a restart shows the
  // same authoritative state rather than an empty form that implies nothing was ever stored.
  const refresh = useCallback(async () => {
    setLoading(true);
    const result = await listProjects();
    setProjects(Array.isArray(result) ? result : []);
    setLoading(false);
  }, []);

  // Recovery first, then the list: a damaged database should be announced rather than rendered as an
  // empty Control Room that looks like a fresh install.
  useEffect(() => {
    void (async () => {
      const report = await recoveryStatus();
      // A transport failure is not a clean report. Treating "could not check" as "nothing wrong" is exactly
      // the silent-success mistake the authority boundary exists to prevent.
      setRecovery("clean" in report ? report : null);
      await refresh();
    })();
  }, [refresh]);

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
    if (check.status === "AUTHORIZED" && check.canonicalPath && check.derivedProjectName) {
      dispatchWorkspace({
        type: "authorized",
        canonicalPath: check.canonicalPath,
        derivedProjectName: check.derivedProjectName,
      });
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
      local_path: authorized ? workspace.canonicalPath : draft.localPath,
      initial_brief: draft.initialBrief,
    });

    if (isProjectView(result)) {
      // Only the returned projection crosses into state. The draft is not sent along.
      dispatch({ type: "accepted", project: result });
      // Re-read rather than trusting the local value: the list and the panel come from one authority, so a
      // stale ordering cannot creep in between them.
      void refresh();
      return;
    }
    dispatch({ type: "rejected", error: result });
  }, [draft, authorized, workspace, refresh]);

  // Create requires an authorized workspace. A candidate or an invalid path cannot be submitted, so a
  // string the UI merely holds can never become a project workspace root.
  const canSubmit = useMemo(
    () =>
      !pending && authorized && draft.initialBrief.trim() !== "",
    [pending, authorized, draft],
  );

  return (
    <main style={{ fontFamily: "system-ui", padding: 32, maxWidth: 760, margin: "0 auto" }}>
      <h1 style={{ marginBottom: 4 }}>Mayasaba</h1>
      <p style={{ color: "#6b7280", marginTop: 0 }}>
        Windows local-first control plane for coordinating CLI coding agents.
      </p>

      {created ? (
        <>
          <ProjectPanel project={created} onStartAnother={() => { setDraft(EMPTY_DRAFT); dispatchWorkspace({ type: "edit", requestedPath: "" }); dispatch({ type: "edit", draft: EMPTY_DRAFT }); }} />
          <ProjectList projects={projects.filter((p) => p.projectId !== created.projectId)} loading={loading} />
        </>
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
      {recovery && !recovery.clean ? <RecoveryBanner report={recovery} /> : null}
      {!created ? <ProjectList projects={projects} loading={loading} /> : null}
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
        Choose the folder where Mayasaba will work, then describe what you want it to accomplish.
      </p>

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

        {authorized ? (
          <div style={{ ...derivedBox }}>
            <span style={derivedLabel}>Project</span>
            <strong style={{ display: "block", marginTop: 2 }}>{workspace.derivedProjectName}</strong>
            <span style={{ ...hint, display: "block", marginTop: 2 }}>
              Taken from the selected folder. Mayasaba names the project after its workspace.
            </span>
          </div>
        ) : null}
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
          &checkmark; Workspace verified: <code>{workspace.canonicalPath}</code>
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

/** Renders stored state only. */
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

/**
 * Startup recovery findings.
 *
 * Deliberately prominent. If the durable store disagrees with itself, the Control Room must say so instead of
 * rendering a plausible-looking empty list, because an empty list is indistinguishable from a fresh install.
 */
function RecoveryBanner({ report }: { report: RecoveryReport }) {
  return (
    <section
      role="alert"
      style={{ border: "1px solid #b91c1c", borderRadius: 6, padding: "12px 14px", marginBottom: 16 }}
    >
      <strong>Stored data needs attention</strong>
      <p style={{ margin: "6px 0 0", fontSize: 13 }}>
        Mayasaba found {report.issues.length} problem(s) in its local database. Nothing has been changed
        automatically — the stored project state is authoritative and only an owning service may repair it.
      </p>
      <ul style={{ margin: "8px 0 0", paddingLeft: 18, fontSize: 13 }}>
        {report.issues.map((issue, i) => (
          <li key={i}>
            <code>{issue.kind}</code> — {issue.detail}
          </li>
        ))}
      </ul>
    </section>
  );
}

/**
 * Persisted projects, read back from Rust.
 *
 * This is the rehydration surface. Nothing here is reconstructed from anything the UI held; every field comes
 * from `list_projects`, so a restart shows the same authoritative state the backend persisted.
 */
function ProjectList({ projects, loading }: { projects: ProjectView[]; loading: boolean }) {
  if (loading) {
    return (
      <p style={hint} role="status">
        Loading persisted projects…
      </p>
    );
  }
  if (projects.length === 0) {
    return (
      <p style={hint}>
        No projects stored yet. Anything you create is written to the local database and will still be here
        after a restart.
      </p>
    );
  }
  return (
    <section aria-label="Projects" style={{ marginTop: 32 }}>
      <h3 style={{ marginBottom: 4 }}>Stored projects</h3>
      <p style={{ ...hint, marginTop: 0 }}>Rehydrated from the local database, newest first.</p>
      <ul style={{ listStyle: "none", padding: 0, margin: "10px 0 0" }}>
        {projects.map((p) => (
          <li
            key={p.projectId}
            style={{ border: "1px solid #e5e7eb", borderRadius: 6, padding: "10px 12px", marginBottom: 8 }}
          >
            <div style={{ display: "flex", justifyContent: "space-between", gap: 12, flexWrap: "wrap" }}>
              <strong>{p.name}</strong>
              <span style={hint}>
                {p.phase} · {p.status} · epoch {p.currentEpoch}
              </span>
            </div>
            <div style={{ ...hint, marginTop: 4, fontFamily: "ui-monospace, monospace" }}>{p.localPath}</div>
            {p.briefBody ? (
              <div style={{ ...hint, marginTop: 4 }}>Brief v{p.briefVersion ?? 1}: {p.briefBody}</div>
            ) : null}
          </li>
        ))}
      </ul>
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
const derivedBox: React.CSSProperties = {
  marginTop: 10,
  padding: "8px 10px",
  border: "1px solid #e5e7eb",
  borderRadius: 6,
  background: "#f9fafb",
  fontSize: 14,
};
const derivedLabel: React.CSSProperties = {
  color: "#6b7280",
  fontSize: 12,
  textTransform: "uppercase",
  letterSpacing: 0.4,
};
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