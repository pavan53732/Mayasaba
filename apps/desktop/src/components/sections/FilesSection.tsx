import { useCallback, useEffect, useReducer, useState } from "react";

import { Button } from "../ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";
import { StatusChip } from "../shell/StatusChip";
import { AttachmentTrayView } from "../shell/AttachmentTray";
import {
  attachProjectContextAttachment,
  listProjectContextAttachments,
  pickAttachmentFiles,
  pickAttachmentFolder,
  resolveProjectContextAttachment,
} from "../../intake/bridge";
import {
  emptyTray,
  isDurable,
  presentationOf,
  trayReducer,
} from "../../attachments/state";
import type { CommandError, ProjectView } from "../../intake/state";

/*
 * The Files section: the authorized workspace scope and the context referenced in place.
 *
 * CONTROL-ROOM-DESIGN.md, "Workspace and action boundary": present task-scoped allowed paths within the
 * user-selected local workspace; the whole PC is not an implicit scan scope.
 *
 * This is the one section with a complete live surface today, because all three attachment operations have
 * registered handlers. The tray is seeded from what Rust stores, so a restart shows the project's real
 * context rather than whatever a previous session happened to hold.
 *
 * `rollback_workspace` and `get_workspace_status` are declared and unimplemented, so the checkpoint half of
 * this section is named by `SectionView` rather than rendered here.
 */

export function FilesSection({ project }: { project: ProjectView | null }) {
  const [tray, dispatch] = useReducer(trayReducer, emptyTray("CHAT_COMPOSER"));
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CommandError | null>(null);

  const projectId = project?.project_id ?? null;

  useEffect(() => {
    if (!projectId) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    void (async () => {
      const stored = await listProjectContextAttachments(projectId);
      if (cancelled) return;
      if (isError(stored)) {
        setError(stored);
        dispatch({ type: "cleared" });
      } else {
        setError(null);
        dispatch({ type: "rehydrated", resolutions: stored });
      }
      setLoading(false);
    })();
    return () => {
      cancelled = true;
    };
  }, [projectId]);

  const attachOne = useCallback(
    async (path: string) => {
      if (!projectId) return;
      dispatch({ type: "attaching", requestedPath: path });
      const attached = await attachProjectContextAttachment(projectId, path, "CHAT_COMPOSER");
      if (isError(attached)) {
        dispatch({ type: "refused", requestedPath: path, code: attached.code, message: attached.message });
      } else {
        dispatch({ type: "attached", requestedPath: path, resolution: attached });
      }
    },
    [projectId],
  );

  const onAddFiles = useCallback(async () => {
    setBusy(true);
    for (const path of await pickAttachmentFiles()) await attachOne(path);
    setBusy(false);
  }, [attachOne]);

  const onAddFolder = useCallback(async () => {
    setBusy(true);
    const picked = await pickAttachmentFolder();
    if (picked !== null) await attachOne(picked);
    setBusy(false);
  }, [attachOne]);

  const onRecheck = useCallback(
    async (attachmentId: string, requestedPath: string) => {
      if (!projectId) return;
      setBusy(true);
      const rechecked = await resolveProjectContextAttachment(projectId, attachmentId);
      if (isError(rechecked)) {
        setError(rechecked);
      } else {
        // Resolvability is an observation, so the same attachment can answer RESOLVED now and UNRESOLVED later
        // without having changed. The row is replaced with what the service just answered.
        dispatch({ type: "attached", requestedPath, resolution: rechecked });
      }
      setBusy(false);
    },
    [projectId],
  );

  if (!project) {
    return (
      <Card>
        <CardContent className="p-4">
          <p className="text-xs text-ink-muted">
            No project open. The authorized scope belongs to a project, so there is nothing to show until one
            is open.
          </p>
        </CardContent>
      </Card>
    );
  }

  const durable = tray.entries.filter(isDurable);

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader>
          <CardTitle>Authorized scope</CardTitle>
          <p className="text-xs text-ink-muted">
            The workspace root this project may operate within. It is an authorization boundary, not
            descriptive metadata.
          </p>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <code className="font-mono text-xs break-all">{project.local_path}</code>
            <StatusChip tone="ok" label="Verified workspace" />
          </div>
          <p className="text-xs text-ink-muted">
            Nothing outside this root is scanned or written without approval. Attachments below are references
            within it, recorded in place.
          </p>
        </CardContent>
      </Card>

      <AttachmentTrayView
        tray={tray}
        busy={busy || loading}
        onAddFiles={onAddFiles}
        onAddFolder={onAddFolder}
        onDiscard={(requestedPath) => dispatch({ type: "discarded", requestedPath })}
        note={
          <>
            Recorded references for this project. Each is <strong>referenced in place</strong> — nothing is
            uploaded, copied, indexed or analysed, and attaching reads no content.
          </>
        }
      />

      {error ? (
        <p role="alert" className="text-xs text-state-danger">
          <strong>{error.code}</strong> — {error.message}
        </p>
      ) : null}

      {durable.length > 0 ? (
        <Card>
          <CardHeader>
            <CardTitle>Re-check stored references</CardTitle>
            <p className="text-xs text-ink-muted">
              Resolvability is an observation, not a stored field: the same reference can resolve now and not
              later without having changed. Re-checking never rewrites the row.
            </p>
          </CardHeader>
          <CardContent>
            <ul className="flex flex-col divide-y divide-border">
              {durable.map((entry, index) => {
                const shown = presentationOf(entry);
                const attachmentId = entry.kind === "attached" ? entry.resolution.attachment.attachment_id : null;
                return (
                  <li
                    key={`${shown.path}#${index}`}
                    className="flex flex-wrap items-center justify-between gap-2 py-2"
                  >
                    <div className="flex min-w-0 flex-col gap-0.5">
                      <code className="font-mono text-xs break-all">{shown.path}</code>
                      <span className="text-[11px] text-ink-muted">
                        {shown.recordedState} · {shown.verdict}
                      </span>
                    </div>
                    {attachmentId ? (
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={busy}
                        onClick={() => void onRecheck(attachmentId, shown.path)}
                      >
                        Re-check
                      </Button>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          </CardContent>
        </Card>
      ) : null}
    </div>
  );
}

function isError<T>(value: T | CommandError): value is CommandError {
  return typeof (value as CommandError).code === "string";
}
