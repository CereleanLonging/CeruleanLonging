import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";

export function ImportReviewDialog({ open, title, onClose, children }: {
  open: boolean; title: string; onClose: () => void; children: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    if (!open || !dialog.current) return;
    const element = dialog.current, opener = document.activeElement;
    element.showModal();
    return () => {
      element.close();
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [open]);
  return open ? createPortal(
    <dialog ref={dialog} className="import-review-dialog" aria-label={title}
      onCancel={event => { event.preventDefault(); onClose(); }}>
      <div className="import-review-heading"><h3>{title}</h3><button type="button" className="import-close" aria-label="关闭导入预览" onClick={onClose}>×</button></div>
      {children}
    </dialog>, document.body,
  ) : null;
}
