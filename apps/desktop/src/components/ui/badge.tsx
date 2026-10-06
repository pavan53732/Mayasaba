import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "../../lib/utils";

/*
 * A status chip.
 *
 * DEC-032 permits frosted-glass accents on status chips and nowhere else, and requires that visual language
 * never encodes state on its own. Every variant therefore carries a text label supplied by the caller; the
 * colour is redundant reinforcement, not the message.
 */
const badgeVariants = cva(
  "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium",
  {
    variants: {
      variant: {
        neutral: "border-border bg-surface-sunken text-ink-muted",
        ok: "border-state-ok/30 bg-state-ok-surface text-state-ok",
        warn: "border-state-warn/30 bg-state-warn-surface text-state-warn",
        danger: "border-state-danger/30 bg-state-danger-surface text-state-danger",
        info: "border-state-info/30 bg-state-info-surface text-state-info",
        frosted: "frosted border-border-strong text-ink",
      },
    },
    defaultVariants: { variant: "neutral" },
  },
);

function Badge({
  className,
  variant,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
