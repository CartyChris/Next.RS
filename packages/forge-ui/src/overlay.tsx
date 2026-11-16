import { cloneElement, useEffect, useId, useRef, useState } from "react";
import type { KeyboardEvent, ReactElement, ReactNode } from "react";
import { cx, getAdapter } from "./config";
import { Icon } from "./icon";
import type { ButtonSize, ButtonVariant } from "./primitives";

export interface MenuAction {
  label: string;
  onSelect?: () => void;
  /** Navigate instead of (or after) `onSelect`. */
  href?: string;
  icon?: ReactNode;
  /** A hint such as `"⌘K"`. Wire the key itself with `useHotkey`. */
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  separator?: false;
}
export type MenuItem = MenuAction | { separator: true };

export interface MenuProps {
  /** The button's content. */
  label: ReactNode;
  /** The button's accessible name, when `label` is only an icon. */
  ariaLabel?: string;
  items: readonly MenuItem[];
  /** Which edge of the button the menu lines up with. */
  align?: "start" | "end";
  variant?: ButtonVariant;
  size?: ButtonSize;
  className?: string;
}

/** A button that opens a list of actions. Arrow keys, Home/End, type-ahead and Escape work. */
export function Menu({ label, ariaLabel, items, align = "start", variant = "secondary", size = "md", className }: MenuProps) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const focusOnOpen = useRef<"first" | "last">("first");
  const typed = useRef({ text: "", at: 0 });

  const enabledItems = () =>
    Array.from(root.current?.querySelectorAll<HTMLElement>('[role="menuitem"]:not([aria-disabled="true"])') ?? []);

  useEffect(() => {
    if (!open) return;
    const all = enabledItems();
    (focusOnOpen.current === "last" ? all[all.length - 1] : all[0])?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [open]);

  const close = (refocus = true) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  };

  const select = (item: MenuAction) => {
    if (item.disabled) return;
    close();
    item.onSelect?.();
    if (item.href) {
      const { navigate, href } = getAdapter();
      if (navigate) navigate(item.href);
      else window.location.assign(href ? href(item.href) : item.href);
    }
  };

  const onButtonKeyDown = (event: KeyboardEvent) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      focusOnOpen.current = event.key === "ArrowUp" ? "last" : "first";
      setOpen(true);
    }
  };

  const onMenuKeyDown = (event: KeyboardEvent) => {
    const all = enabledItems();
    const at = all.indexOf(document.activeElement as HTMLElement);
    let next: HTMLElement | undefined;
    if (event.key === "ArrowDown") next = all[(at + 1) % all.length];
    else if (event.key === "ArrowUp") next = all[(at - 1 + all.length) % all.length];
    else if (event.key === "Home") next = all[0];
    else if (event.key === "End") next = all[all.length - 1];
    else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      return close();
    } else if (event.key === "Tab") return close(false);
    else if (event.key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      const now = Date.now();
      typed.current = { text: (now - typed.current.at > 600 ? "" : typed.current.text) + event.key.toLowerCase(), at: now };
      const rotated = [...all.slice(at + 1), ...all.slice(0, at + 1)];
      next = rotated.find((el) => el.textContent?.trim().toLowerCase().startsWith(typed.current.text));
    }
    if (next) {
      event.preventDefault();
      next.focus();
    }
  };

  return (
    <div ref={root} className={cx("forge-ui-menu", className)} data-align={align}>
      <button ref={button} type="button" className={`forge-ui-button forge-ui-button--${variant}`} data-size={size}
        aria-haspopup="menu" aria-expanded={open} aria-controls={open ? `${id}-menu` : undefined} aria-label={ariaLabel}
        onClick={() => {
          focusOnOpen.current = "first";
          setOpen((value) => !value);
        }}
        onKeyDown={onButtonKeyDown}>
        {label}
        <Icon name="chevron-down" size={14} />
      </button>
      {open && (
        <div id={`${id}-menu`} className="forge-ui-menu-list" role="menu" aria-label={ariaLabel ?? (typeof label === "string" ? label : undefined)}
          onKeyDown={onMenuKeyDown}>
          {items.map((item, index) => item.separator ? (
            <div key={index} className="forge-ui-menu-separator" role="separator" />
          ) : (
            <div key={index} className="forge-ui-menu-item" role="menuitem" tabIndex={-1}
              aria-disabled={item.disabled || undefined} data-danger={item.danger || undefined}
              onClick={() => select(item)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  select(item);
                }
              }}>
              {item.icon != null && <span className="forge-ui-menu-icon" aria-hidden="true">{item.icon}</span>}
              <span className="forge-ui-menu-label">{item.label}</span>
              {item.shortcut && <kbd className="forge-ui-menu-shortcut">{item.shortcut}</kbd>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export interface TooltipProps {
  /** Short supplementary text. Essential information belongs in visible text instead. */
  content: ReactNode;
  /** One focusable element, such as a button or link. */
  children: ReactElement<{ "aria-describedby"?: string }>;
  side?: "top" | "bottom";
  className?: string;
}

/** A hint shown on hover and keyboard focus. Escape hides it. */
export function Tooltip({ content, children, side = "top", className }: TooltipProps) {
  const id = useId();
  const [dismissed, setDismissed] = useState(false);
  const describedBy = [children.props["aria-describedby"], id].filter(Boolean).join(" ");
  return (
    <span className={cx("forge-ui-tooltip-anchor", className)} data-side={side} data-dismissed={dismissed || undefined}
      onKeyDown={(event) => {
        if (event.key === "Escape") setDismissed(true);
      }}
      onPointerLeave={() => setDismissed(false)} onBlur={() => setDismissed(false)}>
      {cloneElement(children, { "aria-describedby": describedBy })}
      <span id={id} role="tooltip" className="forge-ui-tooltip">{content}</span>
    </span>
  );
}

export interface PopoverProps {
  /** The trigger button's content. */
  label: ReactNode;
  ariaLabel?: string;
  children: ReactNode;
  align?: "start" | "end";
  variant?: ButtonVariant;
  size?: ButtonSize;
  className?: string;
}

/**
 * A button that opens a small floating panel (filters, pickers, details). Built on the
 * native popover: click outside or Escape closes it, and focus returns to the button.
 */
export function Popover({ label, ariaLabel, children, align = "start", variant = "secondary", size = "md", className }: PopoverProps) {
  const id = useId();
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);

  const place = () => {
    const anchor = button.current?.getBoundingClientRect();
    const el = panel.current;
    if (!anchor || !el) return;
    const view = el.ownerDocument.documentElement;
    const width = el.offsetWidth;
    const left = align === "end" ? anchor.right - width : anchor.left;
    const below = anchor.bottom + 6;
    const fitsBelow = below + el.offsetHeight <= view.clientHeight - 8;
    el.style.left = `${Math.max(8, Math.min(left, view.clientWidth - width - 8))}px`;
    el.style.top = `${fitsBelow ? below : Math.max(8, anchor.top - el.offsetHeight - 6)}px`;
  };

  return (
    <>
      <button ref={button} type="button" className={`forge-ui-button forge-ui-button--${variant}`} data-size={size}
        popoverTarget={id} aria-label={ariaLabel}>
        {label}
      </button>
      <div ref={panel} id={id} popover="auto" className={cx("forge-ui-popover", className)}
        onToggle={(event) => {
          if (event.newState === "open") place();
        }}>
        {children}
      </div>
    </>
  );
}
