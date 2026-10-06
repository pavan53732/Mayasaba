import * as React from "react";
import { CircleDashed, Info } from "lucide-react";

import { Card, CardContent, CardHeader, CardTitle } from "../ui/card";
import { StatusChip } from "./StatusChip";
import { implementedIn, unimplementedIn, type Operation, type Section } from "../../lib/operations";

/*
 * A Control Room section.
 *
 * Every section renders from the same partition, so no section can present a surface the shell cannot answer.
 * The shape is deliberate:
 *
 *   - the operations the shell can call are listed as live, with the owning service named;
 *   - the operations the contract declares and no handler implements are listed as declared-but-unimplemented,
 *     with the owning service named and the reason stated.
 *
 * The second list is the honest part. The bridge contract deliberately leads the implementation - it declares
 * operations that no handler implements yet - so a section that rendered an empty panel for them would be
 * indistinguishable from a section whose data happens to be empty. Naming them makes the gap legible as
 * unfinished work.
 *
 * Nothing here fabricates a value. A section with no live operation renders no data at all, and says so.
 */

export function SectionView({
  section,
  children,
}: {
  section: Section;
  /** The live surface for this section, when one exists. */
  children?: React.ReactNode;
}) {
  const live = implementedIn(section);
  const pending = unimplementedIn(section);

  return (
    <div className="flex flex-col gap-4">
      <header className="flex flex-col gap-1">
        <div className="flex flex-wrap items-center gap-2">
          <h1 className="text-lg font-semibold tracking-tight">{section.label}</h1>
          <StatusChip
            tone={live.length > 0 ? "ok" : "neutral"}
            label={`${live.length} of ${section.operations.length} live`}
          />
        </div>
        <p className="max-w-3xl text-xs leading-relaxed text-ink-muted">{section.purpose}</p>
      </header>

      {children}

      {live.length > 0 ? (
        <Card>
          <CardHeader>
            <CardTitle>Wired operations</CardTitle>
            <p className="text-xs text-ink-muted">
              These have a registered handler in the Tauri shell, so this section can call them.
            </p>
          </CardHeader>
          <CardContent>
            <OperationList operations={live} />
          </CardContent>
        </Card>
      ) : null}

      {pending.length > 0 ? (
        <Card>
          <CardHeader>
            <CardTitle>Declared, not implemented</CardTitle>
            <p className="text-xs text-ink-muted">
              The bridge contract declares these and the shell registers no handler for them, so this section
              cannot read or change anything through them. They are listed rather than rendered as an empty
              panel, because an empty panel is indistinguishable from a section whose data is genuinely empty.
            </p>
          </CardHeader>
          <CardContent>
            <OperationList operations={pending} muted />
          </CardContent>
        </Card>
      ) : null}

      {live.length === 0 ? (
        <Card>
          <CardContent className="flex items-start gap-2 p-4">
            <Info className="mt-0.5 size-4 shrink-0 text-ink-subtle" aria-hidden />
            <p className="text-xs leading-relaxed text-ink-muted">
              Nothing in this section is wired yet. The Control Room renders no placeholder data for it: the
              authoritative state belongs to Rust, and inventing a value here would be presenting authority the
              UI never received.
            </p>
          </CardContent>
        </Card>
      ) : null}
    </div>
  );
}

function OperationList({ operations, muted = false }: { operations: Operation[]; muted?: boolean }) {
  return (
    <ul className="flex flex-col divide-y divide-border">
      {operations.map((operation) => (
        <li key={operation.name} className="flex flex-wrap items-baseline justify-between gap-2 py-2">
          <div className="flex items-center gap-2">
            {muted ? (
              <CircleDashed className="size-3.5 shrink-0 text-ink-subtle" aria-hidden />
            ) : (
              <span
                className="size-1.5 shrink-0 rounded-full bg-state-ok"
                aria-hidden
                title="A handler is registered"
              />
            )}
            <code className={muted ? "font-mono text-xs text-ink-muted" : "font-mono text-xs"}>
              {operation.name}
            </code>
            <span className="text-[11px] text-ink-subtle">{operation.kind}</span>
          </div>
          <span className="text-[11px] text-ink-muted">{operation.owner}</span>
        </li>
      ))}
    </ul>
  );
}
