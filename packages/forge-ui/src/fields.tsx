import { useId, useState } from "react";
import type { ComponentPropsWithRef, ReactNode } from "react";

export interface FieldContent {
  label: ReactNode;
  description?: ReactNode;
  error?: ReactNode;
}

export interface TextInputProps extends FieldContent, Omit<ComponentPropsWithRef<"input">, "children"> {}
export interface SelectProps extends FieldContent, ComponentPropsWithRef<"select"> {}
export interface TextareaProps extends FieldContent, Omit<ComponentPropsWithRef<"textarea">, "children"> {}
export interface CheckboxProps extends FieldContent, Omit<ComponentPropsWithRef<"input">, "type" | "children"> {}

function hasMessage(value: ReactNode) {
  return value != null && typeof value !== "boolean" && value !== "";
}

function useField(id: string | undefined, describedBy: string | undefined, description: ReactNode, error: ReactNode) {
  const generatedId = useId();
  const fieldId = id ?? generatedId;
  const descriptionId = hasMessage(description) ? `${fieldId}-description` : undefined;
  const errorId = hasMessage(error) ? `${fieldId}-error` : undefined;
  return {
    fieldId,
    descriptionId,
    errorId,
    describedBy: [descriptionId, errorId, describedBy].filter(Boolean).join(" ") || undefined,
  };
}

interface FieldProps extends FieldContent {
  ids: ReturnType<typeof useField>;
  required?: boolean;
  checkbox?: boolean;
  children: ReactNode;
}

function Field({ ids, label, description, error, required, checkbox, children }: FieldProps) {
  return (
    <div className="forge-ui-field" data-invalid={Boolean(ids.errorId) || undefined}
      data-checkbox={checkbox || undefined}>
      <label className="forge-ui-field-label" htmlFor={ids.fieldId}>
        {label}
        {required && <span className="forge-ui-field-required" aria-hidden="true"> *</span>}
      </label>
      {children}
      {ids.descriptionId && <div className="forge-ui-field-description" id={ids.descriptionId}>{description}</div>}
      {ids.errorId && <div className="forge-ui-field-error" id={ids.errorId}>{error}</div>}
    </div>
  );
}

export function TextInput({ id, label, description, error, className = "", ...props }: TextInputProps) {
  const ids = useField(id, props["aria-describedby"], description, error);
  return (
    <Field ids={ids} label={label} description={description} error={error} required={props.required}>
      <input {...props} id={ids.fieldId} className={`forge-ui-control ${className}`}
        aria-describedby={ids.describedBy} aria-invalid={ids.errorId ? true : props["aria-invalid"]} />
    </Field>
  );
}

export function Select({ id, label, description, error, className = "", ...props }: SelectProps) {
  const ids = useField(id, props["aria-describedby"], description, error);
  return (
    <Field ids={ids} label={label} description={description} error={error} required={props.required}>
      <select {...props} id={ids.fieldId} className={`forge-ui-control ${className}`}
        aria-describedby={ids.describedBy} aria-invalid={ids.errorId ? true : props["aria-invalid"]} />
    </Field>
  );
}

export function Textarea({ id, label, description, error, className = "", ...props }: TextareaProps) {
  const ids = useField(id, props["aria-describedby"], description, error);
  return (
    <Field ids={ids} label={label} description={description} error={error} required={props.required}>
      <textarea {...props} id={ids.fieldId} className={`forge-ui-control ${className}`}
        aria-describedby={ids.describedBy} aria-invalid={ids.errorId ? true : props["aria-invalid"]} />
    </Field>
  );
}

export function Checkbox({ id, label, description, error, className = "", ...props }: CheckboxProps) {
  const ids = useField(id, props["aria-describedby"], description, error);
  return (
    <Field ids={ids} label={label} description={description} error={error} required={props.required} checkbox>
      <input {...props} type="checkbox" id={ids.fieldId} className={`forge-ui-checkbox ${className}`}
        aria-describedby={ids.describedBy} aria-invalid={ids.errorId ? true : props["aria-invalid"]} />
    </Field>
  );
}

export interface SwitchProps extends FieldContent, Omit<ComponentPropsWithRef<"input">, "type" | "children"> {}

/** An on/off setting that applies immediately. Use `Checkbox` for choices submitted with a form. */
export function Switch({ id, label, description, error, className = "", ...props }: SwitchProps) {
  const ids = useField(id, props["aria-describedby"], description, error);
  return (
    <Field ids={ids} label={label} description={description} error={error} required={props.required} checkbox>
      <input {...props} type="checkbox" role="switch" id={ids.fieldId} className={`forge-ui-switch ${className}`}
        aria-describedby={ids.describedBy} aria-invalid={ids.errorId ? true : props["aria-invalid"]} />
    </Field>
  );
}

export interface ChoiceOption {
  value: string;
  label: ReactNode;
  description?: ReactNode;
  disabled?: boolean;
}

interface ChoiceGroupProps extends FieldContent {
  /** Shared by the radio inputs; also the form field name. */
  name: string;
  options: readonly ChoiceOption[];
  value?: string;
  defaultValue?: string;
  onValueChange?: (value: string) => void;
  required?: boolean;
  disabled?: boolean;
  className?: string;
}

export interface RadioGroupProps extends ChoiceGroupProps {
  orientation?: "vertical" | "horizontal";
}

function useChoice({ value, defaultValue, onValueChange }: ChoiceGroupProps) {
  // Uncontrolled groups leave the checked state to the browser.
  return (option: ChoiceOption) => ({
    checked: value === undefined ? undefined : value === option.value,
    defaultChecked: value === undefined && defaultValue !== undefined ? defaultValue === option.value : undefined,
    onChange: () => onValueChange?.(option.value),
  });
}

/** One choice from a short list, all visible. Arrow keys move between options. */
export function RadioGroup(props: RadioGroupProps) {
  const { name, label, description, error, options, required, disabled, orientation = "vertical", className = "" } = props;
  const id = useId();
  const choice = useChoice(props);
  const describedBy = [hasMessage(description) && `${id}-description`, hasMessage(error) && `${id}-error`]
    .filter(Boolean).join(" ") || undefined;
  return (
    <fieldset className={`forge-ui-fieldset ${className}`} aria-describedby={describedBy} disabled={disabled}
      data-invalid={hasMessage(error) || undefined}>
      <legend className="forge-ui-field-label">
        {label}
        {required && <span className="forge-ui-field-required" aria-hidden="true"> *</span>}
      </legend>
      {hasMessage(description) && <div className="forge-ui-field-description" id={`${id}-description`}>{description}</div>}
      <div className="forge-ui-radio-list" data-orientation={orientation}>
        {options.map((option) => (
          <label key={option.value} className="forge-ui-radio">
            <input type="radio" name={name} value={option.value} required={required} disabled={option.disabled}
              {...choice(option)} />
            <span>
              <span className="forge-ui-radio-label">{option.label}</span>
              {option.description != null && <span className="forge-ui-field-description">{option.description}</span>}
            </span>
          </label>
        ))}
      </div>
      {hasMessage(error) && <div className="forge-ui-field-error" id={`${id}-error`}>{error}</div>}
    </fieldset>
  );
}

export interface SegmentedControlProps extends Omit<ChoiceGroupProps, "description" | "error" | "required"> {
  size?: "sm" | "md";
  /** Hide the visible label (it still names the group for assistive technology). */
  hideLabel?: boolean;
}

/** Two to five mutually exclusive views or modes, shown as one joined control. */
export function SegmentedControl(props: SegmentedControlProps) {
  const { name, label, options, disabled, size = "md", hideLabel, className = "" } = props;
  const choice = useChoice(props);
  return (
    <fieldset className={`forge-ui-fieldset forge-ui-segmented ${className}`} disabled={disabled} data-size={size}>
      <legend className={hideLabel ? "forge-ui-visually-hidden" : "forge-ui-field-label"}>{label}</legend>
      <div className="forge-ui-segmented-track">
        {options.map((option) => (
          <label key={option.value} className="forge-ui-segment">
            <input type="radio" name={name} value={option.value} disabled={option.disabled} {...choice(option)} />
            <span>{option.label}</span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}

export interface SliderProps extends FieldContent, Omit<ComponentPropsWithRef<"input">, "type" | "children"> {
  /** Show the current value beside the label. A function formats it. */
  showValue?: boolean | ((value: number) => ReactNode);
}

/** A number picked from a range. */
export function Slider({ id, label, description, error, showValue, className = "", onChange, ...props }: SliderProps) {
  const ids = useField(id, props["aria-describedby"], description, error);
  const initial = Number(props.value ?? props.defaultValue ?? ((Number(props.min ?? 0) + Number(props.max ?? 100)) / 2));
  const [current, setCurrent] = useState(initial);
  const shown = props.value === undefined ? current : Number(props.value);
  const output = showValue ? (
    <output className="forge-ui-slider-value" htmlFor={ids.fieldId}>
      {typeof showValue === "function" ? showValue(shown) : shown}
    </output>
  ) : null;
  return (
    <Field ids={ids} label={label} description={description} error={error}>
      <div className="forge-ui-slider-row">
        <input {...props} type="range" id={ids.fieldId} className={`forge-ui-slider ${className}`}
          aria-describedby={ids.describedBy}
          onChange={(event) => {
            setCurrent(event.currentTarget.valueAsNumber);
            onChange?.(event);
          }} />
        {output}
      </div>
    </Field>
  );
}
