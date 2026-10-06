import {
  Activity,
  Bot,
  Boxes,
  FileCheck,
  FlaskConical,
  FolderTree,
  Gavel,
  GitBranch,
  Hammer,
  ListChecks,
  MessageSquare,
  Play,
  ScrollText,
  Settings,
  Users,
  Wrench,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { cn } from "../../lib/utils";
import { SECTIONS, implementedIn, unimplementedIn, type Section } from "../../lib/operations";

/*
 * Primary navigation.
 *
 * CONTROL-ROOM-DESIGN.md, "Layout" lists the 15 sections in order, and `SECTIONS` in `lib/operations.ts`
 * carries that order. The list is not repeated here: this file maps each section id to an icon and renders
 * whatever the registry declares, so a section cannot exist in the navigation without naming the operations
 * it is built from.
 *
 * Each entry states how many of its operations the shell can actually call. That count is the honest part of
 * this surface: a section whose operations are all declared-but-unimplemented is marked as such in the
 * navigation, so a reader is not sent into a panel that cannot answer.
 */

const ICONS: Record<string, LucideIcon> = {
  chat: MessageSquare,
  council: Users,
  requirements: ListChecks,
  architecture: Boxes,
  decisions: Gavel,
  tasks: GitBranch,
  agents: Bot,
  files: FolderTree,
  build: Hammer,
  run: Play,
  tests: FlaskConical,
  repairs: Wrench,
  logs: ScrollText,
  evidence: FileCheck,
  settings: Settings,
};

export function PrimaryNav({
  active,
  onSelect,
}: {
  active: string;
  onSelect: (id: string) => void;
}) {
  return (
    <nav aria-label="Primary" className="flex flex-col gap-0.5 p-2">
      <p className="px-2 pb-1 pt-2 text-[11px] font-medium uppercase tracking-wide text-ink-subtle">
        Sections
      </p>
      {SECTIONS.map((section) => (
        <NavItem
          key={section.id}
          section={section}
          active={section.id === active}
          onSelect={() => onSelect(section.id)}
        />
      ))}
    </nav>
  );
}

function NavItem({
  section,
  active,
  onSelect,
}: {
  section: Section;
  active: boolean;
  onSelect: () => void;
}) {
  const Icon = ICONS[section.id] ?? Activity;
  const live = implementedIn(section).length;
  const declared = section.operations.length;
  const allUnimplemented = live === 0;

  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left text-sm transition-colors",
        active ? "bg-surface text-ink shadow-sm" : "text-ink-muted hover:bg-surface/60 hover:text-ink",
      )}
    >
      <Icon className="size-4 shrink-0" aria-hidden />
      <span className="flex-1 truncate">{section.label}</span>
      <span
        className={cn(
          "shrink-0 font-mono text-[10px]",
          allUnimplemented ? "text-ink-subtle" : "text-ink-muted",
        )}
        title={
          allUnimplemented
            ? `All ${declared} declared operation(s) here have no registered handler`
            : `${live} of ${declared} declared operation(s) have a registered handler`
        }
      >
        {live}/{declared}
      </span>
    </button>
  );
}

/** The count of declared operations the shell cannot call, for the navigation footer. */
export function NavFooter() {
  const total = SECTIONS.reduce((sum, section) => sum + unimplementedIn(section).length, 0);
  return (
    <p className="px-3 py-2 text-[11px] leading-relaxed text-ink-subtle">
      {total} declared operation(s) in these sections have no registered handler. Each section names them
      rather than rendering an empty panel.
    </p>
  );
}
