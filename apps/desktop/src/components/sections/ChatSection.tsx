import { useCallback, useEffect, useReducer, useState } from "react";

import { Card, CardContent } from "../ui/card";
import { OngoingChatComposer } from "../shell/Composers";
import {
  attachProjectContextAttachment,
  listProjectContextAttachments,
  pickAttachmentFiles,
  pickAttachmentFolder,
  recordUserContribution,
} from "../../intake/bridge";
import { canSendMessage, chatReducer, initialChatState } from "../../chat/state";
import type { CommandError, ProjectView } from "../../intake/state";

/*
 * The Chat section: free text recorded as a `UserContribution`.
 *
 * INTERNAL-APPLICATION-ARCHITECTURE.md:309 — free-text input becomes a `UserContribution` with an advisory
 * classification, and the owning service decides whether project truth changes. It is never a second
 * project-creation path.
 *
 * The composer is the same component the project view renders, so the two surfaces cannot drift: there is one
 * implementation of the chat surface and this section supplies it with state.
 *
 * The classification sent is `COMMENTARY` and the stored `result_type` is `PENDING`, because nothing routes
 * the text to an owning service that could rule on it. That is what the record says: the user contributed
 * this text and it was labelled for routing. It does not claim project truth changed, and the notice names
 * the unchanged epoch pair so the surface cannot imply otherwise.
 */

export function ChatSection({ project }: { project: ProjectView | null }) {
  const [state, dispatch] = useReducer(chatReducer, "CHAT_COMPOSER", initialChatState);
  const [loadingAttachments, setLoadingAttachments] = useState(true);

  const projectId = project?.project_id ?? null;

  // Seed the tray from what Rust stores, so a restart shows the project's real context rather than whatever a
  // previous session happened to hold.
  useEffect(() => {
    if (!projectId) {
      setLoadingAttachments(false);
      return;
    }
    let cancelled = false;
    void (async () => {
      setLoadingAttachments(true);
      const stored = await listProjectContextAttachments(projectId);
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
  }, [projectId]);

  const attachOne = useCallback(
    async (path: string) => {
      if (!projectId) return;
      dispatch({ type: "attachment", action: { type: "attaching", requestedPath: path } });
      const attached = await attachProjectContextAttachment(projectId, path, "CHAT_COMPOSER");
      if (isError(attached)) {
        dispatch({
          type: "attachment",
          action: { type: "refused", requestedPath: path, code: attached.code, message: attached.message },
        });
      } else {
        dispatch({
          type: "attachment",
          action: { type: "attached", requestedPath: path, resolution: attached },
        });
      }
    },
    [projectId],
  );

  const onAddFiles = useCallback(async () => {
    for (const path of await pickAttachmentFiles()) await attachOne(path);
  }, [attachOne]);

  const onAddFolder = useCallback(async () => {
    const picked = await pickAttachmentFolder();
    if (picked !== null) await attachOne(picked);
  }, [attachOne]);

  const onSend = useCallback(async () => {
    if (!projectId) return;
    const body = state.text;
    dispatch({ type: "submit" });
    // The advisory label for routing, and nothing more. Nothing on this side decides materiality: the owning
    // service does, and this value is never read as authorization.
    const recorded = await recordUserContribution(projectId, body, "COMMENTARY");
    if (isError(recorded)) {
      dispatch({ type: "refused", error: recorded });
    } else {
      dispatch({ type: "recorded", contribution: recorded });
    }
  }, [projectId, state.text]);

  if (!project) {
    return (
      <Card>
        <CardContent className="p-4">
          <p className="text-xs text-ink-muted">
            No project open. A contribution is recorded against a project, so there is nothing to record until
            one is open.
          </p>
        </CardContent>
      </Card>
    );
  }

  return (
    <OngoingChatComposer
      projectId={project.project_id}
      state={state}
      loadingAttachments={loadingAttachments}
      canSend={canSendMessage(state)}
      onEdit={(text) => dispatch({ type: "edit", text })}
      onSend={() => void onSend()}
      onAddFiles={() => void onAddFiles()}
      onAddFolder={() => void onAddFolder()}
      onDiscard={(requestedPath) =>
        dispatch({ type: "attachment", action: { type: "discarded", requestedPath } })
      }
    />
  );
}

function isError<T>(value: T | CommandError): value is CommandError {
  return typeof (value as CommandError).code === "string";
}
