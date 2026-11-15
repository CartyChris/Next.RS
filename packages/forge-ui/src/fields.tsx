import { useId } from "react";
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
