import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { cx, getAdapter } from "./config";
import { Icon } from "./icon";

export interface Command {
  /** Unique across the palette. */
  id: string;
  label: string;
  /** Secondary text on the right, such as a section or a shortcut. */
  hint?: string;
  /** Commands with the same group are listed under one heading. */
  group?: string;
  /** Extra words to match while searching. */
  keywords?: string;
  icon?: ReactNode;
  onSelect?: () => void;
  /** Navigate to this path when chosen. */
  href?: string;
}

export interface CommandPaletteProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  commands: readonly Command[];
  label?: string;
  placeholder?: string;
  empty?: ReactNode;
  className?: string;
}

/** Characters of `query` appear in order in `text`; earlier and adjacent matches score higher. */
function score(text: string, query: string): number {
  if (!query) return 1;
  let total = 0;
  let from = 0;
  for (const char of query) {
    const at = text.indexOf(char, from);
    if (at < 0) return 0;
    total += at === from ? 3 : at === 0 || text[at - 1] === " " ? 2 : 1;
    from = at + 1;
  }
  return total / (1 + text.length / 100);
}

/**
 * A searchable list of actions and destinations, usually opened with `useHotkey("mod+k", …)`.
 * Type to filter, arrow keys to move, Enter to run, Escape to close.
 */
export function CommandPalette({
  open, onOpenChange, commands, label = "Command palette", placeholder = "Type a command or search…",
  empty = "No matching commands.", className,
}: CommandPaletteProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  const opener = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const el = dialog.current;
    if (!el) return;
    if (open && !el.open) {
      const focused = el.ownerDocument.activeElement;
      opener.current = focused instanceof HTMLElement ? focused : null;
      el.showModal();
      el.querySelector("input")?.focus();
    } else if (!open && el.open) {
      el.close();
      opener.current?.focus({ preventScroll: true });
    }
  }, [open]);

  const run = (command: Command) => {
    onOpenChange(false);
    command.onSelect?.();
    if (command.href) {
      const { navigate, href } = getAdapter();
      if (navigate) navigate(command.href);
      else window.location.assign(href ? href(command.href) : command.href);
    }
  };

  return (
    <dialog ref={dialog} className={cx("forge-ui-dialog forge-ui-command", className)} aria-label={label}
      onCancel={(event) => {
        event.preventDefault();
        onOpenChange(false);
      }}
      onClick={(event) => {
        // A click on the backdrop lands on the dialog element itself.
        if (event.target === event.currentTarget) onOpenChange(false);
      }}>
      {/* Mounted per opening, so the search always starts empty. */}
      {open && <PaletteBody commands={commands} label={label} placeholder={placeholder} empty={empty} run={run} />}
    </dialog>
  );
}

interface PaletteBodyProps {
  commands: readonly Command[];
  label: string;
  placeholder: string;
  empty: ReactNode;
  run: (command: Command) => void;
}

function PaletteBody({ commands, label, placeholder, empty, run }: PaletteBodyProps) {
  const id = useId();
  const list = useRef<HTMLDivElement>(null);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);

  const results = useMemo(() => {
    const q = query.trim().toLowerCase();
    return commands
      .map((command, order) => ({
        command, order,
        rank: score(`${command.label} ${command.keywords ?? ""} ${command.group ?? ""}`.toLowerCase(), q),
      }))
      .filter((entry) => entry.rank > 0)
      // Without a query, keep the author's order (and so their groups) intact.
      .sort((a, b) => (q ? b.rank - a.rank : 0) || a.order - b.order)
      .map((entry) => entry.command);
  }, [commands, query]);
  const current = Math.min(active, Math.max(0, results.length - 1));

  useEffect(() => {
    list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [current, results]);

  let lastGroup: string | undefined;
  return (
    <>
      <div className="forge-ui-command-search">
        <Icon name="search" size={18} />
        <input className="forge-ui-command-input" type="text" role="combobox" aria-expanded="true"
          aria-controls={`${id}-list`} aria-autocomplete="list" aria-label={label} placeholder={placeholder}
          aria-activedescendant={results[current] ? `${id}-${results[current].id}` : undefined}
          autoComplete="off" spellCheck={false} value={query}
          onChange={(event) => {
            setQuery(event.currentTarget.value);
            setActive(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              const step = event.key === "ArrowDown" ? 1 : -1;
              setActive(results.length ? (current + step + results.length) % results.length : 0);
            } else if (event.key === "Home" && results.length) setActive(0);
            else if (event.key === "End" && results.length) setActive(results.length - 1);
            else if (event.key === "Enter") {
              event.preventDefault();
              if (results[current]) run(results[current]);
            }
          }} />
      </div>
      <div ref={list} id={`${id}-list`} className="forge-ui-command-list" role="listbox" aria-label={label}>
        {results.length === 0 && <div className="forge-ui-command-empty" role="presentation">{empty}</div>}
        {results.map((command, index) => {
          const heading = command.group !== lastGroup && command.group && !query.trim() ? command.group : undefined;
          lastGroup = command.group;
          return (
            <div key={command.id} role="presentation">
              {heading && <div className="forge-ui-command-group" role="presentation">{heading}</div>}
              <div id={`${id}-${command.id}`} className="forge-ui-command-item" role="option"
                aria-selected={index === current} onPointerMove={() => setActive(index)} onClick={() => run(command)}>
                {command.icon != null && <span className="forge-ui-menu-icon" aria-hidden="true">{command.icon}</span>}
                <span className="forge-ui-menu-label">{command.label}</span>
                {command.hint && <span className="forge-ui-command-hint">{command.hint}</span>}
              </div>
            </div>
          );
        })}
      </div>
    </>
  );
}
