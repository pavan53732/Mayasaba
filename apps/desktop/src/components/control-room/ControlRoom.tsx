import { useCallback, useEffect, useMemo, useReducer, useState } from "react";

import { Header } from "../shell/Header";
import { PrimaryNav, NavFooter } from "../shell/PrimaryNav";
import { ContextRail } from "../shell/ContextRail";
import { SectionView } from "../shell/SectionView";
import { ProjectList, ProjectPanel, RecoveryBanner } from "../shell/ProjectPanel";
import { UnattachedSelections } from "../shell/AttachmentTray";
import { Composer } from "../shell/Composers";
import { ChatSection } from "../sections/ChatSection";
import { FilesSection } from "../sections/FilesSection";
import { LogsSection } from "../sections/LogsSection";
import { AgentsSection } from "../sections/AgentsSection";
import { ScrollArea } from "../ui/scroll-area";
import { TooltipProvider } from "../ui/tooltip";
import { sectionById } from "../../lib/operations";
import {
  attachProjectContextAttachment,
  createProject,
  getRecoveryStatus,
  listProjects,
  pickAttachmentFiles,
  pickAttachmentFolder,
  pickFolder,
  validateWorkspace,
} from "../../intake/bridge";
import {
  emptyTray,
  queuedPaths,
  trayReducer,
  type AttachmentResolution,
} from "../../attachments/state";
import {
  EMPTY_DRAFT,
  emptyWorkspace,
  initialState,
  intakeReducer,
  isAuthorized,
  isSubmitting,
  workspaceReducer,
  type CommandError,
  type Draft,
  type ProjectView,
  type RecoveryReport,
} from "../../intake/state";

/*
 * The Mayasaba Control Room.
 *
 * CONTROL-ROOM-DESIGN.md, "Layout": a persistent shell of header, primary navigation, center content and a
 * right context rail, composed as a bento grid (DEC-032). This component is that composition.
 *
 * React owns presentation and draft state. Rust owns project truth. Nothing here constructs a project from a
 * form, spawns a process, touches the filesystem or reads SQLite: every authoritative value arrives through a
 * typed Tauri command and is rendered as it arrived (AGENTS.md section 11).
 *
 * The shell holds no lifecycle state machine of its own. `intakeReducer` and `workspaceReducer` are the
 * existing interaction-state reducers, unchanged; the project's phase, status and epoch are rendered from the
 * projection Rust returned and are never advanced locally.
 *
 * The center content is the selected section. A section with no live operation renders its declared-but-
 * unimplemented operations rather than placeholder data, so the shell never presents authority it did not
 * receive.
 */

export default function ControlRoom() {
  const [state, dispatch] = useReducer(intakeReducer, initialState);
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [workspace, dispatchWorkspace] = useReducer(workspaceReducer, emptyWorkspace);
  // The intake surface's attachment tray. Selections are queued here while the user is still composing,
  // because an attachment is project-scoped and there is no project id to attach to yet (DEC-106, DEC-107).
  const [intakeTray, dispatchTray] = useReducer(trayReducer, emptyTray("INITIAL_INTAKE_COMPOSER"));
  const [projects, setProjects] = useState<ProjectView[]>([]);
  const [loading, setLoading] = useState(true);
  const [recovery, setRecovery] = useState<RecoveryReport | null>(null);
  const [active, setActive] = useState("chat");
  // The project the Control Room is showing. It is either the one just created or one opened from the stored
  // list, and it is always a projection Rust returned - never a project reconstructed from the form.
  const [openProject, setOpenProject] = useState<ProjectView | null>(null);

  // Rehydrate on launch. Everything rendered for an existing project comes from Rust, so a restart shows the
  // same authoritative state rather than an empty form that implies nothing was ever stored.
  const refresh = useCallback(async () => {
    setLoading(true);
    const result = await listProjects();
    setProjects(Array.isArray(result) ? result : []);
    setLoading(false);
  }, []);

  // Recovery first, then the list: a damaged database should be announced rather than rendered as an empty
  // Control Room that looks like a fresh install.
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

  // The project the shell renders. A freshly created project takes precedence over an opened one, because the
  // creation result is the newest authoritative projection.
  const project = created ?? openProject;

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
      setOpenProject(null);
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
  const canSubmit = useMemo(() => !pending && authorized && draft.initialBrief.trim() !== "", [
    pending,
    authorized,
    draft,
  ]);

  const onStartAnother = useCallback(() => {
    setDraft(EMPTY_DRAFT);
    dispatchWorkspace({ type: "edit", requestedPath: "" });
    dispatch({ type: "edit", draft: EMPTY_DRAFT });
    // A tray describes one project's context, and a stored row must never be displayed against the wrong
    // project.
    dispatchTray({ type: "cleared" });
    setOpenProject(null);
  }, []);

  const section = sectionById(active);

  return (
    <TooltipProvider delayDuration={300}>
      <div className="flex h-full min-h-0 flex-col bg-surface-sunken">
        <Header project={project} recovery={recovery} loading={loading} onRefresh={() => void refresh()} />

        {/* The bento-grid composition DEC-032 requires: a fixed navigation column, a fluid center, and a
            fixed context rail, all sharing one gap unit so the panels read as tiles rather than as a page. */}
        <div className="grid min-h-0 flex-1 grid-cols-[220px_minmax(0,1fr)_300px] gap-bento p-bento">
          <div className="min-h-0 overflow-hidden rounded-bento border border-border bg-surface-raised">
            <ScrollArea className="h-full">
              <PrimaryNav active={active} onSelect={setActive} />
              <NavFooter />
            </ScrollArea>
          </div>

          <main className="min-h-0 overflow-hidden rounded-bento border border-border bg-surface">
            <ScrollArea className="h-full">
              <div className="flex flex-col gap-4 p-4">
                {recovery && !recovery.clean ? <RecoveryBanner report={recovery} /> : null}

                {project ? (
                  <>
                    <ProjectPanel project={project} onStartAnother={onStartAnother} />
                    <UnattachedSelections tray={intakeTray} />
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
                    onSubmit={() => void onSubmit()}
                    error={error}
                    tray={intakeTray}
                    onAddFiles={() => void onAddAttachmentFiles()}
                    onAddFolder={() => void onAddAttachmentFolder()}
                    onDiscard={onDiscardAttachment}
                  />
                )}

                <SectionView section={section}>{renderSection(section.id, project)}</SectionView>

                {!project ? (
                  <ProjectList
                    projects={projects}
                    loading={loading}
                    onOpen={(p) => {
                      setOpenProject(p);
                      dispatch({ type: "edit", draft: EMPTY_DRAFT });
                    }}
                  />
                ) : (
                  <ProjectList
                    projects={projects.filter((p) => p.project_id !== project.project_id)}
                    loading={loading}
                    onOpen={setOpenProject}
                  />
                )}
              </div>
            </ScrollArea>
          </main>

          <div className="min-h-0 overflow-hidden rounded-bento border border-border">
            <ContextRail project={project} />
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
}

/**
 * The live surface for a section, when one exists.
 *
 * Only the sections whose operations have registered handlers render anything here. Every other section
 * returns null and `SectionView` renders its declared-but-unimplemented operations instead, so there is no
 * path by which a section shows a value the shell cannot read.
 */
function renderSection(id: string, project: ProjectView | null) {
  switch (id) {
    case "chat":
      return <ChatSection project={project} />;
    case "files":
      return <FilesSection project={project} />;
    case "logs":
      return <LogsSection project={project} />;
    case "agents":
      return <AgentsSection project={project} />;
    default:
      return null;
  }
}

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
