import { Button } from "../ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Textarea } from "../ui/textarea";
import { StatusChip } from "./StatusChip";
import { AttachmentTrayView } from "./AttachmentTray";
import { isAuthorized, type CommandError, type Draft, type WorkspaceState } from "../../intake/state";
import type { ChatState } from "../../chat/state";
import type { AttachmentTray } from "../../attachments/state";

/*
 * The Initial Intake Composer.
 *
 * React owns presentation and draft state. Rust owns project truth. This component never constructs a project
 * from the form - after a successful command the shell renders only the projection the service returned,
 * because the accepted state contains no draft to render.
 *
 * The styling moved from inline `React.CSSProperties` to Tailwind classes and the shadcn primitives. Every
 * behavioural claim in the original comments still holds, and the two that are load-bearing are restated
 * below because they are the reason this surface is shaped the way it is.
 */

export function Composer(props: {
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
    <Card>
      <CardHeader>
        <CardTitle className="text-base">New project</CardTitle>
        <p className="text-xs text-ink-muted">
          Choose the folder where Mayasaba will work, then describe what you want it to accomplish.
        </p>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="workspace-path">Local workspace</Label>
          <div className="flex gap-2">
            <Input
              id="workspace-path"
              value={
                workspace.kind === "authorized"
                  ? workspace.canonicalPath
                  : workspace.kind === "candidate" || workspace.kind === "invalid"
                    ? workspace.requestedPath
                    : ""
              }
              disabled={pending || selecting}
              onChange={(e) => onEditWorkspace(e.target.value)}
            />
            <Button
              type="button"
              variant="outline"
              className="whitespace-nowrap"
              onClick={onBrowse}
              disabled={pending || selecting}
            >
              {selecting ? "Opening…" : "Browse…"}
            </Button>
          </div>
          <WorkspaceStatus workspace={workspace} />

          {authorized ? (
            <div className="mt-1 rounded-md border border-border bg-surface-sunken px-2.5 py-2">
              <span className="text-[11px] uppercase tracking-wide text-ink-subtle">Project</span>
              <strong className="mt-0.5 block text-sm">{workspace.derivedProjectName}</strong>
              <span className="mt-0.5 block text-xs text-ink-muted">
                Taken from the selected folder. Mayasaba names the project after its workspace.
              </span>
            </div>
          ) : null}
        </div>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="initial-brief">Describe the project</Label>
          <Textarea
            id="initial-brief"
            value={draft.initialBrief}
            disabled={pending}
            rows={6}
            onChange={(e) => onChange({ ...draft, initialBrief: e.target.value })}
            className="resize-y"
          />
          <p className="text-xs leading-relaxed text-ink-muted">
            This becomes <strong>ProjectBrief version 1</strong> — the canonical record of your project intent.
            It stays a local draft until the project is created.
          </p>
        </div>

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

        <div className="flex flex-col gap-2">
          <div>
            <Button onClick={onSubmit} disabled={!canSubmit}>
              {pending ? "Creating…" : "Create project"}
            </Button>
          </div>
          {!authorized ? (
            <p className="text-xs text-ink-muted">
              Select a local folder to continue. A path must be verified before it can be a workspace.
            </p>
          ) : null}

          {pending ? (
            <p role="status" className="rounded-md border border-border px-3 py-2.5 text-xs">
              Submitting. Nothing is stored yet.
            </p>
          ) : null}

          {error ? (
            <div
              role="alert"
              className="rounded-md border border-state-danger/40 bg-state-danger-surface px-3 py-2.5 text-xs text-state-danger"
            >
              <strong>{error.code}</strong>
              <div className="mt-1">{error.message}</div>
              <div className="mt-1.5 text-ink-muted">Your draft has been kept.</div>
            </div>
          ) : null}
        </div>
      </CardContent>
    </Card>
  );
}

/** Shows what Rust decided about the workspace. It never infers authorization from the text in the field. */
function WorkspaceStatus({ workspace }: { workspace: WorkspaceState }) {
  switch (workspace.kind) {
    case "authorized":
      return (
        <p className="mt-1.5 text-xs text-state-ok">
          &checkmark; Workspace verified: <code className="font-mono">{workspace.canonicalPath}</code>
        </p>
      );
    case "candidate":
      return (
        <p className="mt-1.5 text-xs text-ink-muted">
          Checking this folder… it is not a workspace until Mayasaba verifies it.
        </p>
      );
    case "invalid":
      return (
        <p role="alert" className="mt-1.5 text-xs text-state-danger">
          <strong>{workspace.code}</strong> — {workspace.message}
        </p>
      );
    case "selecting":
      return <p className="mt-1.5 text-xs text-ink-muted">Choose a folder…</p>;
    case "empty":
      return (
        <p className="mt-1.5 text-xs text-ink-muted">
          A folder Mayasaba may operate within. Nothing is authorized until it is verified.
        </p>
      );
  }
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
export function OngoingChatComposer(props: {
  projectId: string;
  state: ChatState;
  loadingAttachments: boolean;
  canSend: boolean;
  onEdit: (text: string) => void;
  onSend: () => void;
  onAddFiles: () => void;
  onAddFolder: () => void;
  onDiscard: (requestedPath: string) => void;
}) {
  const { state, loadingAttachments, canSend, onEdit, onSend, onAddFiles, onAddFolder, onDiscard } = props;

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Add to this project</CardTitle>
        <p className="text-xs text-ink-muted">
          Free text is recorded as a <strong>UserContribution</strong> with an advisory classification. Only
          the owning service decides whether it changes project truth; a message never mutates state on its
          own.
        </p>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="chat-message">Message</Label>
          <Textarea
            id="chat-message"
            value={state.text}
            rows={4}
            onChange={(event) => onEdit(event.target.value)}
            className="resize-y"
          />
        </div>

        <AttachmentTrayView
          tray={state.tray}
          busy={loadingAttachments}
          onAddFiles={onAddFiles}
          onAddFolder={onAddFolder}
          onDiscard={onDiscard}
          note={
            <>
              Optional. These are the project's recorded references — from intake and from chat — each
              <strong> referenced in place</strong>. Nothing here is uploaded, copied, indexed or analysed.
            </>
          }
        />

        <div className="flex flex-col gap-2">
          <div>
            <Button onClick={onSend} disabled={!canSend}>
              Send
            </Button>
          </div>
          <p className="text-xs text-ink-muted">
            Attachments are not a prerequisite for sending. This control is enabled by the message text alone,
            so it is available with nothing attached and with references that no longer resolve.
          </p>

          {state.kind === "recorded" ? (
            <div
              role="status"
              className="rounded-md border border-state-ok/40 bg-state-ok-surface px-3 py-2.5 text-xs text-state-ok"
            >
              <strong>Recorded.</strong>
              <div className="mt-1">
                Stored as <code className="font-mono">{state.contribution.contribution_id}</code> with the
                advisory classification <code className="font-mono">{state.contribution.classification}</code>.
                No owning service has ruled on it, so the outcome is{" "}
                <code className="font-mono">{state.contribution.result_type}</code> and project truth is
                unchanged — the epoch pair is{" "}
                <code className="font-mono">
                  {state.contribution.epoch_before} → {state.contribution.epoch_after}
                </code>
                . The label above is advisory; it never authorized anything.
              </div>
            </div>
          ) : null}

          {state.kind === "refused" ? (
            <div
              role="alert"
              className="rounded-md border border-state-warn/40 bg-state-warn-surface px-3 py-2.5 text-xs text-state-warn"
            >
              <strong>Not recorded.</strong>
              <div className="mt-1">
                <code className="font-mono">{state.error.code}</code> — {state.error.message} Your draft and
                references are kept.
              </div>
            </div>
          ) : null}
        </div>
      </CardContent>
    </Card>
  );
}

/** A status chip for the intake surface's own state, so the shell and the composer agree on presentation. */
export function IntakeStatus({ pending, authorized }: { pending: boolean; authorized: boolean }) {
  if (pending) return <StatusChip tone="info" label="Submitting" />;
  if (authorized) return <StatusChip tone="ok" label="Workspace verified" />;
  return <StatusChip tone="neutral" label="Draft" />;
}
