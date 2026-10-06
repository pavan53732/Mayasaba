import * as React from "react";

import { Badge } from "../ui/badge";

/*
 * A status chip.
 *
 * DEC-032 permits frosted-glass accents on the header bar and on status chips, and requires that visual
 * language never encode state on its own. Both constraints meet here: the chip always renders a text label,
 * and the colour is redundant reinforcement of that label rather than the message. A reader who cannot
 * distinguish the colours still reads the state.
 *
 * `tone` is a presentation choice made by the caller from a value Rust returned. It is never derived from a
 * value the UI computed, and it never gates an action - enablement is advisory and Rust authorization is
 * authoritative (CONTROL-ROOM-DESIGN.md, "Action semantics").
 */

export type Tone = "neutral" | "ok" | "warn" | "danger" | "info";

export function StatusChip({
  tone = "neutral",
  label,
  detail,
  icon,
  className,
}: {
  tone?: Tone;
  label: string;
  detail?: string;
  icon?: React.ReactNode;
  className?: string;
}) {
  return (
    <Badge variant={tone} className={className} title={detail}>
      {icon}
      <span>{label}</span>
      {detail ? <span className="text-ink-subtle">· {detail}</span> : null}
    </Badge>
  );
}

/**
 * A labelled key/value pair for the context rail.
 *
 * The value is rendered as it arrived. A missing value renders as an explicit "not reported" rather than as a
 * dash or a zero, because a dash reads as "nothing to see" and a zero reads as a measurement.
 */
export function RailField({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: React.ReactNode;
  mono?: boolean;
}) {
  const absent = value === null || value === undefined || value === "";
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="text-[11px] uppercase tracking-wide text-ink-subtle">{label}</dt>
      <dd className={mono && !absent ? "font-mono text-xs break-all" : "text-xs"}>
        {absent ? <span className="text-ink-subtle italic">not reported</span> : value}
      </dd>
    </div>
  );
}
