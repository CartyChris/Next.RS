import { useSyncExternalStore } from "react";
import type { Tone } from "./primitives";

export interface Notification {
  id: string;
  title: string;
  description?: string;
  tone?: Tone;
}

export interface NotificationRegionProps {
  notifications: readonly Notification[];
  onDismiss?: (id: string) => void;
  label?: string;
  className?: string;
}

export function NotificationRegion({
  notifications, onDismiss, label = "Notifications", className = "",
}: NotificationRegionProps) {
  return (
    <div className={`forge-ui-notifications ${className}`}
      role="log" aria-label={label} aria-live="polite" aria-relevant="additions">
      {notifications.map(({ id, title, description, tone = "info" }) => (
        <div key={id} className="forge-ui-notification" data-tone={tone}>
          <div>
            <div className="forge-ui-notification-title">{title}</div>
            {description && <div className="forge-ui-notification-description">{description}</div>}
          </div>
          {onDismiss && (
            <button type="button" className="forge-ui-button forge-ui-button--ghost forge-ui-notification-dismiss"
              aria-label={`Dismiss ${title}`} onClick={() => onDismiss(id)}>
              <span aria-hidden="true">×</span>
            </button>
          )}
        </div>
      ))}
    </div>
  );
}

// ---- toast(): notifications without wiring state by hand ----

export interface ToastOptions {
  title: string;
  description?: string;
  tone?: Tone;
  /** Milliseconds before it disappears. `0` keeps it until dismissed. Default 5000. */
  duration?: number;
  /** Reuse an id to replace a toast that is still showing. */
  id?: string;
}

const EMPTY: readonly Notification[] = [];
let toasts: readonly Notification[] = EMPTY;
let nextId = 0;
const listeners = new Set<() => void>();
const timers = new Map<string, ReturnType<typeof setTimeout>>();

function emit(next: readonly Notification[]) {
  toasts = next;
  for (const listener of listeners) listener();
}

/** Show a notification in the mounted `<Toaster />`. Returns its id. */
export function toast(input: string | ToastOptions): string {
  const options = typeof input === "string" ? { title: input } : input;
  const id = options.id ?? `toast-${++nextId}`;
  const { duration = 5000, ...notification } = options;
  clearTimeout(timers.get(id));
  emit([...toasts.filter((item) => item.id !== id), { ...notification, id }]);
  if (duration > 0 && typeof window !== "undefined") {
    timers.set(id, setTimeout(() => dismissToast(id), duration));
  }
  return id;
}

/** Remove one toast, or all of them when called without an id. */
export function dismissToast(id?: string): void {
  if (id === undefined) {
    for (const timer of timers.values()) clearTimeout(timer);
    timers.clear();
    emit(EMPTY);
    return;
  }
  clearTimeout(timers.get(id));
  timers.delete(id);
  emit(toasts.filter((item) => item.id !== id));
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export interface ToasterProps {
  label?: string;
  className?: string;
}

/** Mount once (in the root layout, inside `Theme`), then call `toast()` from anywhere. */
export function Toaster({ label, className }: ToasterProps) {
  const current = useSyncExternalStore(subscribe, () => toasts, () => EMPTY);
  return <NotificationRegion notifications={current} onDismiss={dismissToast} label={label} className={className} />;
}
