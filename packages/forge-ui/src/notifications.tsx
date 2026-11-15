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
