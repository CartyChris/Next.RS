import type { ComponentPropsWithRef, ReactNode } from "react";
import type { Tone } from "./primitives";

export type HudPosition =
  | "top-left" | "top" | "top-right" | "left" | "center" | "right" | "bottom-left" | "bottom" | "bottom-right";

export interface HudProps extends ComponentPropsWithRef<"div"> {
  children: ReactNode;
}

/**
 * An overlay for a game canvas: put it in the same positioned box as the canvas and fill it
 * with `HudSlot`s. Clicks pass through to the game except on interactive slots.
 */
export function Hud({ className = "", ...props }: HudProps) {
  return <div {...props} className={`forge-ui-hud ${className}`} />;
}

export interface HudSlotProps extends ComponentPropsWithRef<"div"> {
  position: HudPosition;
  /** Let this slot receive pointer events (buttons, menus). */
  interactive?: boolean;
  /** Draw a translucent panel behind the content. */
  panel?: boolean;
}

export function HudSlot({ position, interactive, panel, className = "", ...props }: HudSlotProps) {
  return <div {...props} className={`forge-ui-hud-slot ${className}`} data-position={position}
    data-interactive={interactive || undefined} data-panel={panel || undefined} />;
}

export interface BarProps extends Omit<ComponentPropsWithRef<"div">, "children"> {
  /** What the bar measures, e.g. `"Health"`. */
  label: string;
  value: number;
  max?: number;
  tone?: Exclude<Tone, "neutral"> | "accent";
  /** Show `value / max` beside the label. */
  showValue?: boolean;
}

/** A gauge for a known range: health, energy, capacity, quota. */
export function Bar({ label, value, max = 100, tone = "success", showValue, className = "", ...props }: BarProps) {
  const ratio = Math.min(1, Math.max(0, value / max));
  return (
    <div {...props} className={`forge-ui-bar ${className}`} data-tone={tone}>
      <div className="forge-ui-bar-head" aria-hidden="true">
        <span>{label}</span>
        {showValue && <span>{Math.round(value)} / {max}</span>}
      </div>
      <div className="forge-ui-progress-track" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={max}
        aria-valuenow={value}>
        <div className="forge-ui-progress-fill" style={{ inlineSize: `${ratio * 100}%` }} />
      </div>
    </div>
  );
}
