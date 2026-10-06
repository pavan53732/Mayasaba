import * as React from "react";

import { Button } from "../ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";
import { StatusChip } from "./StatusChip";
import type { ProjectView, RecoveryReport } from "../../intake/state";

/*
 * The project surfaces the persistent shell owns.
 *
 * These were inline-styled in `App.tsx` before the Control Room shell existed. The behaviour is unchanged -
 * every field still comes from the projection Rust returned, and nothing here reconstructs a project from
 * anything the UI held - but the styling moved to Tailwind classes and the shadcn primitives so the shell and
 * the sections share one visual language (DEC-014, DEC-032).
 */

/** A definition-list term. */
export const Term = ({ children }: { children: React.ReactNode }) => (
  <dt className="text-xs text-ink-muted">{children}</dt>
);

/** A definition-list value. Monospaced, because every value here is an identifier or a path. */
export const Value = ({ children }: { children: React.ReactNode }) => (
  <dd className="font-mono text-xs break-all">{children}</dd>
);

/** Renders stored state only. */
export function ProjectPanel({
  project,
  onStartAnother,
}: {
  project: ProjectView;
  onStartAnother: () => void;
}) {
  return (
    <Card>
      <CardHeader>
        <p className="text-xs text-ink-muted">Persisted project state</p>
        <CardTitle className="text-base">{project.name}</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <dl className="grid grid-cols-[160px_1fr] gap-x-3 gap-y-2">
          <Term>Project ID</Term>
          <Value>{project.project_id}</Value>
          <Term>Workspace</Term>
          <Value>{project.local_path}</Value>
          <Term>Phase</Term>
          <Value>{project.phase}</Value>
          <Term>Status</Term>
          <Value>{project.status}</Value>
          <Term>Epoch</Term>
          <Value>{project.current_epoch}</Value>
          <Term>Brief ID</Term>
          <Value>{project.brief_id ?? "—"}</Value>
          <Term>Brief version</Term>
          <Value>{project.brief_version ?? "—"}</Value>
        </dl>

        <div>
          <h3 className="mb-1 text-sm font-semibold">Project brief v{project.brief_version ?? 1}</h3>
          <p className="whitespace-pre-wrap text-sm">{project.brief_body ?? "—"}</p>
        </div>

        <div>
          <Button variant="outline" onClick={onStartAnother}>
            New project
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

/**
 * Startup recovery findings.
 *
 * Deliberately prominent. If the durable store disagrees with itself, the Control Room must say so instead of
 * rendering a plausible-looking empty list, because an empty list is indistinguishable from a fresh install.
 */
export function RecoveryBanner({ report }: { report: RecoveryReport }) {
  return (
    <section
      role="alert"
      className="rounded-bento border border-state-danger/40 bg-state-danger-surface p-3.5"
    >
      <div className="flex items-center gap-2">
        <StatusChip tone="danger" label="Stored data needs attention" />
      </div>
      <p className="mt-2 text-xs leading-relaxed">
        Mayasaba found {report.issues.length} problem(s) in its local database. Nothing has been changed
        automatically — the stored project state is authoritative and only an owning service may repair it.
      </p>
      <ul className="mt-2 list-disc pl-5 text-xs">
        {report.issues.map((issue, i) => (
          <li key={i}>
            <code className="font-mono">{issue.kind}</code> — {issue.detail}
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
export function ProjectList({
  projects,
  loading,
  onOpen,
}: {
  projects: ProjectView[];
  loading: boolean;
  onOpen?: (project: ProjectView) => void;
}) {
  if (loading) {
    return (
      <p role="status" className="text-xs text-ink-muted">
        Loading persisted projects…
      </p>
    );
  }
  if (projects.length === 0) {
    return (
      <p className="text-xs text-ink-muted">
        No projects stored yet. Anything you create is written to the local database and will still be here
        after a restart.
      </p>
    );
  }
  return (
    <section aria-label="Projects" className="flex flex-col gap-2">
      <div>
        <h3 className="text-sm font-semibold">Stored projects</h3>
        <p className="text-xs text-ink-muted">Rehydrated from the local database, newest first.</p>
      </div>
      <ul className="flex flex-col gap-2">
        {projects.map((p) => (
          <li key={p.project_id}>
            <Card>
              <CardContent className="flex flex-col gap-1 p-3">
                <div className="flex flex-wrap items-baseline justify-between gap-2">
                  <strong className="text-sm">{p.name}</strong>
                  <span className="text-xs text-ink-muted">
                    {p.phase} · {p.status} · epoch {p.current_epoch}
                  </span>
                </div>
                <div className="font-mono text-xs break-all text-ink-muted">{p.local_path}</div>
                {p.brief_body ? (
                  <div className="text-xs text-ink-muted">
                    Brief v{p.brief_version ?? 1}: {p.brief_body}
                  </div>
                ) : null}
                {onOpen ? (
                  <div className="pt-1">
                    <Button variant="outline" size="sm" onClick={() => onOpen(p)}>
                      Open in Control Room
                    </Button>
                  </div>
                ) : null}
              </CardContent>
            </Card>
          </li>
        ))}
      </ul>
    </section>
  );
}
