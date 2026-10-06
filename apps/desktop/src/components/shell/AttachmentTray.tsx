import * as React from "react";

import { Button } from "../ui/button";
import { Card, CardContent } from "../ui/card";
import { StatusChip, type Tone } from "./StatusChip";
import {
  isDurable,
  presentationOf,
  type AttachmentEntry,
  type AttachmentTray,
} from "../../attachments/state";

/*
 * One tray, rendered identically on both surfaces.
 *
 * Every label, state and path comes from `presentationOf`, which reads Rust's answer. This component computes
 * no attachment state of its own, so it has no way to render a selection as attached, or a reference as
 * captured, before the controller said so.
 *
 * The styling moved from inline `React.CSSProperties` to Tailwind classes and the shadcn primitives; the
 * behaviour and every comment's claim are unchanged.
 */

export function AttachmentTrayView(props: {
  tray: AttachmentTray;
  busy: boolean;
  onAddFiles: () => void;
  onAddFolder: () => void;
  onDiscard: (requestedPath: string) => void;
  note: React.ReactNode;
}) {
  const { tray, busy, onAddFiles, onAddFolder, onDiscard, note } = props;
  return (
    <section aria-label="Attachments" className="rounded-bento border border-border bg-surface-sunken p-3.5">
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <strong className="text-sm">Supporting context</strong>
        <span className="text-xs text-ink-muted">Optional — never required</span>
      </div>
      <p className="mt-1 mb-2.5 text-xs leading-relaxed text-ink-muted">{note}</p>

      <div className="flex flex-wrap gap-2">
        <Button type="button" variant="outline" size="sm" onClick={onAddFiles} disabled={busy}>
          Attach files…
        </Button>
        <Button type="button" variant="outline" size="sm" onClick={onAddFolder} disabled={busy}>
          Attach a folder…
        </Button>
      </div>

      {tray.entries.length === 0 ? (
        <p className="mt-2.5 text-xs text-ink-muted">Nothing attached. This is a normal state.</p>
      ) : (
        <ul className="mt-2.5 flex flex-col gap-1.5">
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
    <li>
      <Card>
        <CardContent className="flex flex-col gap-1 p-2.5">
          <div className="flex flex-wrap items-baseline justify-between gap-2.5">
            <code className="font-mono text-xs break-all">{shown.path}</code>
            <span className="whitespace-nowrap text-xs text-ink-muted">{shown.label}</span>
          </div>

          {shown.durable ? (
            <div className="text-xs text-ink-muted">
              Recorded <strong>{shown.recordedState}</strong>
              {" · "}
              {shown.provenance === "INITIAL_INTAKE_COMPOSER" ? "from intake" : "from chat"}
              {" · "}
              {shown.captured ? "contents captured" : "contents not read"}
              {shown.consumed ? " · accepted by an owning service" : ""}
            </div>
          ) : null}

          {shown.verdict === "UNRESOLVED" ? (
            <div className="text-xs text-state-warn">
              The source no longer resolves as recorded{failed?.code ? ` (${failed.code})` : ""}. The reference
              is kept — nothing is deleted.
            </div>
          ) : null}

          {entry.kind === "refused" ? (
            <div className="text-xs text-state-danger">
              <strong>{entry.code}</strong> — {entry.message}
            </div>
          ) : null}

          {entry.kind === "candidate" ? (
            <div>
              <Button
                type="button"
                variant="link"
                size="sm"
                className="h-auto p-0"
                onClick={() => onDiscard(entry.requestedPath)}
                disabled={busy}
              >
                Remove from this list
              </Button>
            </div>
          ) : null}
        </CardContent>
      </Card>
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
export function UnattachedSelections({ tray }: { tray: AttachmentTray }) {
  const unrecorded = tray.entries.filter((entry) => !isDurable(entry));
  if (unrecorded.length === 0) return null;

  return (
    <section
      role="alert"
      className="rounded-bento border border-state-warn/40 bg-state-warn-surface p-3.5"
    >
      <StatusChip tone="warn" label="Some selections were not attached" />
      <p className="mt-2 text-xs leading-relaxed">
        The project was created, but these selections are not stored as context. Nothing was uploaded or
        copied.
      </p>
      <ul className="mt-2 list-disc pl-5 text-xs">
        {unrecorded.map((entry, index) => {
          const shown = presentationOf(entry);
          return (
            <li key={`${shown.path}#${index}`}>
              <code className="font-mono">{shown.path}</code> —{" "}
              {entry.kind === "refused" ? `${entry.code}: ${entry.message}` : shown.label}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** The tone a recorded attachment's state is presented with. Presentation only; the label carries the state. */
export function attachmentTone(entry: AttachmentEntry): Tone {
  const shown = presentationOf(entry);
  if (entry.kind === "refused") return "danger";
  if (shown.verdict === "UNRESOLVED") return "warn";
  if (shown.durable) return "ok";
  return "neutral";
}
