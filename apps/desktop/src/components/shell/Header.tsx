import { RefreshCw, ShieldCheck } from "lucide-react";

import { Button } from "../ui/button";
import { Separator } from "../ui/separator";
import { StatusChip, type Tone } from "./StatusChip";
import type { ProjectView, RecoveryReport } from "../../intake/state";

/*
 * The persistent header.
 *
 * CONTROL-ROOM-DESIGN.md, "Layout": the persistent shell carries project/path, phase/status, agent health,
 * execution status and pause/resume/stop. This is that bar, and it is the only place besides a status chip
 * where DEC-032 permits a frosted-glass accent.
 *
 * Every value here is rendered from what Rust returned. The header holds no lifecycle state of its own: it
 * receives the authoritative projection and displays it, so it cannot become a second state machine
 * (AGENTS.md section 11).
 *
 * The lifecycle controls are rendered disabled with the reason stated, because the operations behind them
 * (`pause_project`, `resume_project`, `stop_project`) are declared by the contract and have no registered
 * handler. A control that looked live and failed at click time would be worse than one that says why it
 * cannot act.
 */

export function Header({
  project,
  recovery,
  loading,
  onRefresh,
}: {
  project: ProjectView | null;
  recovery: RecoveryReport | null;
  loading: boolean;
  onRefresh: () => void;
}) {
  const recoveryTone: Tone = recovery === null ? "neutral" : recovery.clean ? "ok" : "danger";
  const recoveryLabel =
    recovery === null ? "Recovery not reported" : recovery.clean ? "Store clean" : "Store needs attention";

  return (
    <header className="frosted sticky top-0 z-30 border-b border-border">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-2.5">
        <div className="flex items-baseline gap-2">
          <span className="text-sm font-semibold tracking-tight">Mayasaba</span>
          <span className="text-[11px] text-ink-subtle">Control Room</span>
        </div>

        <Separator orientation="vertical" className="h-5" />

        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1.5">
          {project ? (
            <>
              <span className="truncate text-sm font-medium">{project.name}</span>
              <span className="truncate font-mono text-[11px] text-ink-muted" title={project.local_path}>
                {project.local_path}
              </span>
              <StatusChip tone="info" label={project.phase} detail={`epoch ${project.current_epoch}`} />
              <StatusChip tone="neutral" label={project.status} />
            </>
          ) : (
            <span className="text-xs text-ink-muted">
              No project open. Create one from the Intake surface, or open a stored project.
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          <StatusChip
            tone={recoveryTone}
            label={recoveryLabel}
            icon={<ShieldCheck className="size-3" aria-hidden />}
          />
          <StatusChip tone="neutral" label="Agents" detail="not reported" />
          <StatusChip tone="neutral" label="Execution" detail="not reported" />
        </div>

        <Separator orientation="vertical" className="h-5" />

        <div className="flex items-center gap-1.5">
          <LifecycleControl label="Pause" />
          <LifecycleControl label="Resume" />
          <LifecycleControl label="Stop" />
          <Button
            variant="ghost"
            size="icon"
            onClick={onRefresh}
            disabled={loading}
            aria-label="Re-read authoritative state"
            title="Re-read authoritative state from Rust"
          >
            <RefreshCw className={loading ? "animate-spin" : undefined} aria-hidden />
          </Button>
        </div>
      </div>
    </header>
  );
}

/**
 * A lifecycle control whose operation has no handler.
 *
 * Rendered disabled with the reason in the accessible name and the tooltip, rather than omitted. Omitting it
 * would make the shell look like it never intended to offer lifecycle control; rendering it live would make
 * the shell claim an authority it does not have.
 */
function LifecycleControl({ label }: { label: string }) {
  const reason = `${label.toLowerCase()}_project is declared by the bridge contract and has no registered handler, so this control cannot act.`;
  return (
    <Button variant="outline" size="sm" disabled title={reason} aria-label={`${label} — unavailable: ${reason}`}>
      {label}
    </Button>
  );
}
