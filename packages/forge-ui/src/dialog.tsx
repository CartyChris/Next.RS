import { createContext, useCallback, useContext, useEffect, useId, useMemo, useRef } from "react";
import type { ReactNode } from "react";

export interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  className?: string;
}

interface ParentDialog {
  open: boolean;
  show: () => void;
}

const DialogContext = createContext<ParentDialog | null>(null);

export function Dialog({ open, onOpenChange, title, description, children, footer, className }: DialogProps) {
  const id = useId();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);
  const parent = useContext(DialogContext);
  const visible = open && (parent?.open ?? true);

  const show = useCallback(() => {
    const dialog = dialogRef.current;
    if (!visible || !dialog || dialog.open) return;
    // Child effects can run before parent effects on mount. Open the parent first
    // so a nested dialog becomes the topmost modal and keeps the correct opener.
    parent?.show();
    const active = dialog.ownerDocument.activeElement;
    openerRef.current = active instanceof HTMLElement ? active : null;
    dialog.showModal();
  }, [parent, visible]);

  const context = useMemo(() => ({ open: visible, show }), [visible, show]);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (!visible || !dialog) return;
    show();

    return () => {
      if (dialog.open) {
        // A child modal makes everything outside it inert, including this
        // dialog's opener. Close nested modals from the inside out first.
        const descendants = dialog.querySelectorAll<HTMLDialogElement>("dialog.forge-ui-dialog[open]");
        for (let index = descendants.length - 1; index >= 0; index--) {
          descendants[index].close();
        }
        dialog.close();
      }
      const opener = openerRef.current;
      openerRef.current = null;
      if (opener?.isConnected && dialog.ownerDocument.activeElement !== opener) {
        opener.focus({ preventScroll: true });
      }
    };
  }, [show, visible]);

  return (
    <DialogContext.Provider value={context}>
      <dialog
        ref={dialogRef}
        className={className ? `forge-ui-dialog ${className}` : "forge-ui-dialog"}
        aria-modal="true"
        aria-labelledby={`${id}-title`}
        aria-describedby={description == null ? undefined : `${id}-description`}
        onCancel={(event) => {
          if (event.target !== event.currentTarget) return;
          event.preventDefault();
          onOpenChange(false);
        }}
        onClose={(event) => {
          // Native forms with method="dialog" also request a state change.
          // Ignore a queued close event if the modal has already reopened.
          if (event.target === event.currentTarget && open && !event.currentTarget.open) {
            onOpenChange(false);
          }
        }}
      >
        <header className="forge-ui-dialog-header">
          <h2 id={`${id}-title`} className="forge-ui-dialog-title">{title}</h2>
          {description != null && (
            <p id={`${id}-description`} className="forge-ui-dialog-description">{description}</p>
          )}
          <button
            type="button"
            className="forge-ui-button forge-ui-button--ghost forge-ui-dialog-close"
            aria-label="Close dialog"
            onClick={() => onOpenChange(false)}
          >
            <span aria-hidden="true">×</span>
          </button>
        </header>
        <div className="forge-ui-dialog-body">{children}</div>
        {footer != null && <footer className="forge-ui-dialog-footer">{footer}</footer>}
      </dialog>
    </DialogContext.Provider>
  );
}
