import { useId, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";

export interface TabItem {
  /** Unique across this tab list. */
  value: string;
  label: ReactNode;
  content: ReactNode;
  disabled?: boolean;
}

export interface TabsProps {
  items: readonly TabItem[];
  label: string;
  defaultValue?: string;
  value?: string;
  onValueChange?: (value: string) => void;
  className?: string;
}

export function Tabs({ items, label, defaultValue, value, onValueChange, className }: TabsProps) {
  const id = useId();
  const [internalValue, setInternalValue] = useState(defaultValue);
  const buttons = useRef<Array<HTMLButtonElement | null>>([]);
  const enabled = items.flatMap((item, index) => item.disabled ? [] : [index]);
  const selectedIndex = enabled.find((index) => items[index].value === (value ?? internalValue)) ?? enabled[0];

  function select(index: number) {
    if (index === selectedIndex) return;
    const nextValue = items[index].value;
    if (value === undefined) setInternalValue(nextValue);
    onValueChange?.(nextValue);
  }

  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    let next: number | undefined;

    switch (event.key) {
      case "Home":
        next = enabled[0];
        break;
      case "End":
        next = enabled[enabled.length - 1];
        break;
      case "ArrowLeft":
      case "ArrowRight": {
        const view = event.currentTarget.ownerDocument.defaultView;
        const rtl = view?.getComputedStyle(event.currentTarget).direction === "rtl";
        const forward = (event.key === "ArrowRight") !== rtl;
        const position = enabled.indexOf(index);
        next = enabled[(position + (forward ? 1 : -1) + enabled.length) % enabled.length];
        break;
      }
      default:
        return;
    }

    if (next === undefined) return;
    event.preventDefault();
    buttons.current[next]?.focus();
    select(next);
  }

  if (!items.length) return null;

  return (
    <div className={className ? `forge-ui-tabs ${className}` : "forge-ui-tabs"}>
      <div className="forge-ui-tab-list" role="tablist" aria-label={label}>
        {items.map((item, index) => (
          <button
            key={item.value}
            ref={(button) => { buttons.current[index] = button; }}
            id={`${id}-tab-${index}`}
            className="forge-ui-tab"
            type="button"
            role="tab"
            aria-selected={index === selectedIndex}
            aria-controls={`${id}-panel-${index}`}
            tabIndex={index === selectedIndex ? 0 : -1}
            disabled={item.disabled}
            onClick={() => select(index)}
            onKeyDown={(event) => handleKeyDown(event, index)}
          >
            {item.label}
          </button>
        ))}
      </div>
      {items.map((item, index) => (
        <div
          key={item.value}
          id={`${id}-panel-${index}`}
          className="forge-ui-tab-panel"
          role="tabpanel"
          aria-labelledby={`${id}-tab-${index}`}
          hidden={index !== selectedIndex}
          tabIndex={index === selectedIndex ? 0 : undefined}
        >
          {item.content}
        </div>
      ))}
    </div>
  );
}
