import { useCallback, useEffect, useMemo, useReducer, useState } from "react";

import {
  attachProjectContextAttachment,
  createProject,
  getRecoveryStatus,
  listProjectContextAttachments,
  listProjects,
  pickAttachmentFiles,
  pickAttachmentFolder,
  pickFolder,
  recordUserContribution,
  validateWorkspace,
} from "./intake/bridge";
import {
  emptyTray,
  isDurable,
  presentationOf,
  queuedPaths,
  trayReducer,
  type AttachmentEntry,
  type AttachmentResolution,
  type AttachmentTray,
} from "./attachments/state";
import { canSendMessage, chatReducer, initialChatState, type UserContribution } from "./chat/state";
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
  // The intake surface's attachment tray. Selections are queued here while the user is still composing,
  // because an attachment is project-scoped and there is no project id to attach to yet (DEC-106, DEC-107).
  const [intakeTray, dispatchTray] = useReducer(trayReducer, emptyTray("INITIAL_INTAKE_COMPOSER"));
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
      const report = await getRecoveryStatus();
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
    if (check.status === "AUTHORIZED" && check.canonical_path && check.derived_project_name) {
      dispatchWorkspace({
        type: "authorized",
        canonicalPath: check.canonical_path,
        derivedProjectName: check.derived_project_name,
      });
    } else {
      dispatchWorkspace({
        type: "rejected",
        code: check.code ?? "WORKSPACE_NOT_ACCESSIBLE",
        message: check.message ?? "That folder could not be used as a workspace.",
      });
    }
  }, []);

  // Picking an attachment produces candidates and nothing else. Nothing is read, hashed or recorded until the
  // service answers, and while the user is still composing there is no project to record against at all.
  const onAddAttachmentFiles = useCallback(async () => {
    const picked = await pickAttachmentFiles();
    for (const path of picked) dispatchTray({ type: "selected", path });
  }, []);

  const onAddAttachmentFolder = useCallback(async () => {
    const picked = await pickAttachmentFolder();
    if (picked !== null) dispatchTray({ type: "selected", path: picked });
  }, []);

  const onDiscardAttachment = useCallback((requestedPath: string) => {
    dispatchTray({ type: "discarded", requestedPath });
  }, []);

  const onSubmit = useCallback(async () => {
    dispatch({ type: "submit" });
    // The one place the UI's draft vocabulary becomes the wire request: `Draft` is camelCase interaction state
    // and `CreateProjectRequest` mirrors `create_projectRequest` field for field (DEC-054). Naming the two
    // fields here is deliberate - it is the boundary, not a translation layer that has to be kept in step with
    // Rust by hand.
    const result = await createProject({
      local_path: authorized ? workspace.canonicalPath : draft.localPath,
      initial_brief: draft.initialBrief,
    });

    if (isProjectView(result)) {
      // An attachment is project-scoped, so nothing could be recorded until the project existed. The queued
      // selections are attached now, in the order they were picked, and each answer replaces its own entry.
      //
      // A refusal is kept on the entry rather than swallowed. The project was created but that selection was
      // not attached, and the user has to be able to see which one, and why, instead of a tray that quietly
      // looks shorter than what they picked.
      for (const path of queuedPaths(intakeTray)) {
        dispatchTray({ type: "attaching", requestedPath: path });
        const attached = await attachProjectContextAttachment(
          result.project_id,
          path,
          "INITIAL_INTAKE_COMPOSER",
        );
        if (isResolution(attached)) {
          dispatchTray({ type: "attached", requestedPath: path, resolution: attached });
        } else {
          dispatchTray({
            type: "refused",
            requestedPath: path,
            code: attached.code,
            message: attached.message,
          });
        }
      }

      // Only the returned projection crosses into state. The draft is not sent along.
      dispatch({ type: "accepted", project: result });
      // Re-read rather than trusting the local value: the list and the panel come from one authority, so a
      // stale ordering cannot creep in between them.
      void refresh();
      return;
    }
    dispatch({ type: "rejected", error: result });
  }, [draft, authorized, workspace, intakeTray, refresh]);

  // Create requires an authorized workspace. A candidate or an invalid path cannot be submitted, so a
  // string the UI merely holds can never become a project workspace root.
  //
  // The attachment tray is deliberately absent from this expression. Attaching is optional at intake too, and
  // a dependency on the tray here would make a selection a prerequisite for creating a project - the same
  // mistake DEC-106 forbids one surface over.
  const canSubmit = useMemo(
    () =>
      !pending && authorized && draft.initialBrief.trim() !== "",
    [pending, authorized, draft],
  );

  const onStartAnother = useCallback(() => {
    setDraft(EMPTY_DRAFT);
    dispatchWorkspace({ type: "edit", requestedPath: "" });
    dispatch({ type: "edit", draft: EMPTY_DRAFT });
    // A tray describes one project's context. Without this the composer for the second project would render the
    // first project's references, and a stored row must never be displayed against the wrong project.
    dispatchTray({ type: "cleared" });
  }, []);

  return (
    <main style={{ fontFamily: "system-ui", padding: 32, maxWidth: 760, margin: "0 auto" }}>
      <h1 style={{ marginBottom: 4 }}>Mayasaba</h1>
      <p style={{ color: "#6b7280", marginTop: 0 }}>
        Windows local-first control plane for coordinating CLI coding agents.
      </p>

      {created ? (
        <>
          <ProjectPanel project={created} onStartAnother={onStartAnother} />
          <UnattachedSelections tray={intakeTray} />
          <OngoingChatComposer project={created} />
          <ProjectList projects={projects.filter((p) => p.project_id !== created.project_id)} loading={loading} />
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
          tray={intakeTray}
          onAddFiles={onAddAttachmentFiles}
          onAddFolder={onAddAttachmentFolder}
          onDiscard={onDiscardAttachment}
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
  tray: AttachmentTray;
  onAddFiles: () => void;
  onAddFolder: () => void;
  onDiscard: (requestedPath: string) => void;
}) {
  const { draft, onChange, workspace, onBrowse, onEditWorkspace, pending, canSubmit, onSubmit, error } = props;
  const { tray, onAddFiles, onAddFolder, onDiscard } = props;
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

      <AttachmentTrayView
        tray={tray}
        busy={pending}
        onAddFiles={onAddFiles}
        onAddFolder={onAddFolder}
        onDiscard={onDiscard}
        note={
          <>
            Optional supporting context. Files are <strong>referenced in place</strong> — nothing is uploaded,
            copied or read, and nothing is required here to create the project.
          </>
        }
      />

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
        <Term>Project ID</Term><Value>{project.project_id}</Value>
        <Term>Workspace</Term><Value>{project.local_path}</Value>
        <Term>Phase</Term><Value>{project.phase}</Value>
        <Term>Status</Term><Value>{project.status}</Value>
        <Term>Epoch</Term><Value>{project.current_epoch}</Value>
        <Term>Brief ID</Term><Value>{project.brief_id ?? "—"}</Value>
        <Term>Brief version</Term><Value>{project.brief_version ?? "—"}</Value>
      </dl>

      <h3 style={{ marginBottom: 4 }}>Project brief v{project.brief_version ?? 1}</h3>
      <p style={{ whiteSpace: "pre-wrap", marginTop: 0 }}>{project.brief_body ?? "—"}</p>

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
            key={p.project_id}
            style={{ border: "1px solid #e5e7eb", borderRadius: 6, padding: "10px 12px", marginBottom: 8 }}
          >
            <div style={{ display: "flex", justifyContent: "space-between", gap: 12, flexWrap: "wrap" }}>
              <strong>{p.name}</strong>
              <span style={hint}>
                {p.phase} · {p.status} · epoch {p.current_epoch}
              </span>
            </div>
            <div style={{ ...hint, marginTop: 4, fontFamily: "ui-monospace, monospace" }}>{p.local_path}</div>
            {p.brief_body ? (
              <div style={{ ...hint, marginTop: 4 }}>Brief v{p.brief_version ?? 1}: {p.brief_body}</div>
            ) : null}
          </li>
        ))}
      </ul>
    </section>
  );
}

/**
 * One tray, rendered identically on both surfaces.
 *
 * Every label, state and path comes from `presentationOf`, which reads Rust's answer. This component computes
 * no attachment state of its own, so it has no way to render a selection as attached, or a reference as
 * captured, before the controller said so.
 */
function AttachmentTrayView(props: {
  tray: AttachmentTray;
  busy: boolean;
  onAddFiles: () => void;
  onAddFolder: () => void;
  onDiscard: (requestedPath: string) => void;
  note: React.ReactNode;
}) {
  const { tray, busy, onAddFiles, onAddFolder, onDiscard, note } = props;
  return (
    <section aria-label="Attachments" style={trayBox}>
      <div style={{ display: "flex", justifyContent: "space-between", gap: 12, flexWrap: "wrap", alignItems: "baseline" }}>
        <strong style={{ fontSize: 14 }}>Supporting context</strong>
        <span style={hint}>Optional — never required</span>
      </div>
      <p style={{ ...hint, margin: "4px 0 10px" }}>{note}</p>

      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        <button type="button" onClick={onAddFiles} disabled={busy} style={secondaryButton}>
          Attach files…
        </button>
        <button type="button" onClick={onAddFolder} disabled={busy} style={secondaryButton}>
          Attach a folder…
        </button>
      </div>

      {tray.entries.length === 0 ? (
        <p style={{ ...hint, margin: "10px 0 0" }}>Nothing attached. This is a normal state.</p>
      ) : (
        <ul style={{ listStyle: "none", padding: 0, margin: "10px 0 0" }}>
          {tray.entries.map((entry, index) => (
            <AttachmentRow
              key={`${presentationOf(entry).path}#${index}`}
              entry={entry}
              busy={busy}
              onDiscard={onDiscard}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

/** One entry. The label states what is recorded; it never states what the file is used for. */
function AttachmentRow(props: {
  entry: AttachmentEntry;
  busy: boolean;
  onDiscard: (requestedPath: string) => void;
}) {
  const { entry, busy, onDiscard } = props;
  const shown = presentationOf(entry);
  const failed = shown.checks.find((check) => check.status === "FAIL");

  return (
    <li style={trayRow}>
      <div style={{ display: "flex", justifyContent: "space-between", gap: 10, flexWrap: "wrap" }}>
        <code style={{ fontSize: 12, wordBreak: "break-all" }}>{shown.path}</code>
        <span style={{ ...hint, whiteSpace: "nowrap" }}>{shown.label}</span>
      </div>

      {shown.durable ? (
        <div style={{ ...hint, marginTop: 4 }}>
          Recorded <strong>{shown.recordedState}</strong>
          {" · "}
          {shown.provenance === "INITIAL_INTAKE_COMPOSER" ? "from intake" : "from chat"}
          {" · "}
          {shown.captured ? "contents captured" : "contents not read"}
          {shown.consumed ? " · accepted by an owning service" : ""}
        </div>
      ) : null}

      {shown.verdict === "UNRESOLVED" ? (
        <div style={{ ...hint, marginTop: 4, color: "#b45309" }}>
          The source no longer resolves as recorded{failed?.code ? ` (${failed.code})` : ""}. The reference is
          kept — nothing is deleted.
        </div>
      ) : null}

      {entry.kind === "refused" ? (
        <div style={{ ...hint, marginTop: 4, color: "#b91c1c" }}>
          <strong>{entry.code}</strong> — {entry.message}
        </div>
      ) : null}

      {entry.kind === "candidate" ? (
        <button type="button" onClick={() => onDiscard(entry.requestedPath)} disabled={busy} style={linkButton}>
          Remove from this list
        </button>
      ) : null}
    </li>
  );
}

/**
 * The intake selections that were never recorded, shown after the project exists.
 *
 * Recorded ones are deliberately not repeated here: they are part of the project's attachment set and appear
 * in the composer below, read back from Rust. This section exists so that a refusal cannot vanish when the
 * intake composer is replaced by the project view.
 */
function UnattachedSelections({ tray }: { tray: AttachmentTray }) {
  const unrecorded = tray.entries.filter((entry) => !isDurable(entry));
  if (unrecorded.length === 0) return null;

  return (
    <section role="alert" style={{ ...notice, borderColor: "#b45309", marginTop: 16 }}>
      <strong>Some selections were not attached</strong>
      <p style={{ margin: "6px 0 0", fontSize: 13 }}>
        The project was created, but these selections are not stored as context. Nothing was uploaded or copied.
      </p>
      <ul style={{ margin: "8px 0 0", paddingLeft: 18, fontSize: 13 }}>
        {unrecorded.map((entry, index) => {
          const shown = presentationOf(entry);
          return (
            <li key={`${shown.path}#${index}`}>
              <code>{shown.path}</code> —{" "}
              {entry.kind === "refused" ? `${entry.code}: ${entry.message}` : shown.label}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/**
 * The Ongoing Chat Composer: the second surface, available after project creation.
 *
 * INTERNAL-APPLICATION-ARCHITECTURE.md:309 — free-text input becomes a `UserContribution` with an advisory
 * classification, and the owning service decides whether project truth changes. It is never a second
 * project-creation path.
 *
 * The attachment half is wired to the three operations the contract declares: the tray is seeded from the
 * project's stored attachments, and attaching here records immediately, because the project already exists.
 * The message half is wired to `record_user_contribution` (DEC-030), so submitting records a `UserContribution`
 * and the composer renders the row the service stored rather than the draft it sent.
 *
 * The classification sent is `COMMENTARY` and the stored `result_type` is `PENDING`, because nothing routes the
 * text to an owning service that could rule on it. That is what the record says: the user contributed this
 * text and it was labelled for routing. It does not claim project truth changed, and the notice names the
 * unchanged epoch pair so the surface cannot imply otherwise.
 *
 * The enablement rule is unchanged and still the point: `canSendMessage` takes the message text and nothing
 * else, so the send control is available with nothing attached, which is DEC-106's requirement stated on the
 * surface itself rather than only in a test.
 */
function OngoingChatComposer({ project }: { project: ProjectView }) {
  const [state, dispatch] = useReducer(chatReducer, "CHAT_COMPOSER", initialChatState);
  const [loadingAttachments, setLoadingAttachments] = useState(true);

  // Seed the tray from what Rust stores, so a restart shows the project's real context rather than whatever a
  // previous session happened to hold.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      setLoadingAttachments(true);
      const stored = await listProjectContextAttachments(project.project_id);
      if (cancelled) return;
      dispatch({
        type: "attachment",
        action: { type: "rehydrated", resolutions: Array.isArray(stored) ? stored : [] },
      });
      setLoadingAttachments(false);
    })();
    return () => {
      cancelled = true;
    };
  }, [project.project_id]);

  const attachOne = useCallback(
    async (path: string) => {
      dispatch({ type: "attachment", action: { type: "attaching", requestedPath: path } });
      const attached = await attachProjectContextAttachment(
        project.project_id,
        path,
        "CHAT_COMPOSER",
      );
      if (isResolution(attached)) {
        dispatch({
          type: "attachment",
          action: { type: "attached", requestedPath: path, resolution: attached },
        });
      } else {
        dispatch({
          type: "attachment",
          action: { type: "refused", requestedPath: path, code: attached.code, message: attached.message },
        });
      }
    },
    [project.project_id],
  );

  const onAddFiles = useCallback(async () => {
    for (const path of await pickAttachmentFiles()) await attachOne(path);
  }, [attachOne]);

  const onAddFolder = useCallback(async () => {
    const picked = await pickAttachmentFolder();
    if (picked !== null) await attachOne(picked);
  }, [attachOne]);

  const canSend = canSendMessage(state);

  const onSend = useCallback(async () => {
    const body = state.text;
    dispatch({ type: "submit" });
    // The advisory label for routing, and nothing more. Nothing on this side decides materiality: the owning
    // service does, and this value is never read as authorization. It is COMMENTARY because no operation routes
    // the text to a service that could rule on it, and the stored result_type records that rather than implying
    // a change.
    const recorded = await recordUserContribution(project.project_id, body, "COMMENTARY");
    if (isContribution(recorded)) {
      dispatch({ type: "recorded", contribution: recorded });
    } else {
      dispatch({ type: "refused", error: recorded });
    }
  }, [project.project_id, state.text]);

  return (
    <section
      aria-label="Ongoing Chat Composer"
      style={{ marginTop: 24, border: "1px solid #e5e7eb", borderRadius: 8, padding: 20 }}
    >
      <h2 style={{ margin: "0 0 4px" }}>Add to this project</h2>
      <p style={{ ...hint, marginTop: 0 }}>
        Free text is recorded as a <strong>UserContribution</strong> with an advisory classification. Only the
        owning service decides whether it changes project truth; a message never mutates state on its own.
      </p>

      <label style={field}>
        Message
        <textarea
          value={state.text}
          rows={4}
          onChange={(event) => dispatch({ type: "edit", text: event.target.value })}
          style={{ ...input, height: "auto", resize: "vertical" }}
        />
      </label>

      <AttachmentTrayView
        tray={state.tray}
        busy={loadingAttachments}
        onAddFiles={onAddFiles}
        onAddFolder={onAddFolder}
        onDiscard={(requestedPath) =>
          dispatch({ type: "attachment", action: { type: "discarded", requestedPath } })
        }
        note={
          <>
            Optional. These are the project's recorded references — from intake and from chat — each
            <strong> referenced in place</strong>. Nothing here is uploaded, copied, indexed or analysed.
          </>
        }
      />

      <button onClick={onSend} disabled={!canSend} style={button}>
        Send
      </button>
      <p style={hint}>
        Attachments are not a prerequisite for sending. This control is enabled by the message text alone, so it
        is available with nothing attached and with references that no longer resolve.
      </p>

      {state.kind === "recorded" ? (
        <div role="status" style={{ ...notice, borderColor: "#047857", color: "#065f46" }}>
          <strong>Recorded.</strong>
          <div style={{ marginTop: 4 }}>
            Stored as <code>{state.contribution.contribution_id}</code> with the advisory classification{" "}
            <code>{state.contribution.classification}</code>. No owning service has ruled on it, so the outcome is{" "}
            <code>{state.contribution.result_type}</code> and project truth is unchanged — the epoch pair is{" "}
            <code>
              {state.contribution.epoch_before} → {state.contribution.epoch_after}
            </code>
            . The label above is advisory; it never authorized anything.
          </div>
        </div>
      ) : null}

      {state.kind === "refused" ? (
        <div role="alert" style={{ ...notice, borderColor: "#b45309", color: "#92400e" }}>
          <strong>Not recorded.</strong>
          <div style={{ marginTop: 4 }}>
            <code>{state.error.code}</code> — {state.error.message} Your draft and references are kept.
          </div>
        </div>
      ) : null}
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
  return typeof (value as ProjectView).project_id === "string";
}

/**
 * True when the service answered with a stored attachment rather than a refusal.
 *
 * A `CommandError` always carries a string `code` and a resolution never does, so this distinguishes the two
 * by the shape the contract gives each, not by which fields happen to be present.
 */
function isResolution(value: AttachmentResolution | CommandError): value is AttachmentResolution {
  return typeof (value as CommandError).code !== "string";
}

/**
 * True when the service answered with a stored contribution rather than a refusal.
 *
 * A `CommandError` always carries a string `code` and a contribution never does, so this distinguishes the two
 * by the shape the contract gives each, not by which fields happen to be present.
 */
function isContribution(value: UserContribution | CommandError): value is UserContribution {
  return typeof (value as CommandError).code !== "string";
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
const trayBox: React.CSSProperties = {
  border: "1px solid #e5e7eb",
  borderRadius: 6,
  padding: "12px 14px",
  marginBottom: 16,
  background: "#f9fafb",
};
const trayRow: React.CSSProperties = {
  border: "1px solid #e5e7eb",
  borderRadius: 6,
  padding: "8px 10px",
  marginBottom: 6,
  background: "#fff",
};
const secondaryButton: React.CSSProperties = {
  padding: "7px 12px",
  borderRadius: 6,
  border: "1px solid #d1d5db",
  background: "#fff",
  color: "#111827",
  fontSize: 13,
  cursor: "pointer",
};
const linkButton: React.CSSProperties = {
  marginTop: 6,
  padding: 0,
  border: "none",
  background: "none",
  color: "#6b7280",
  fontSize: 12,
  textDecoration: "underline",
  cursor: "pointer",
};