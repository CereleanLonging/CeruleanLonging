import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ApiTarget, ReasoningEffort } from "./contracts";
import { ReasoningOptions, reasoningLabel } from "./ReasoningOptions";

export function ModelOptionsButton({
  model,
  target,
  effort,
  disabled,
  onChange,
  allowDefault = true,
}: {
  model: string;
  target: ApiTarget;
  effort?: ReasoningEffort | null;
  disabled?: boolean;
  onChange: (effort?: ReasoningEffort) => void;
  allowDefault?: boolean;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        className="secondary-button"
        disabled={disabled || !model.trim()}
        onClick={() => setOpen(true)}
      >
        思考强度 · {reasoningLabel(effort)}
      </button>
      {open && (
        <OptionsDialog
          key={JSON.stringify([model, target])}
          model={model}
          target={target}
          effort={effort}
          disabled={disabled}
          allowDefault={allowDefault}
          onClose={() => setOpen(false)}
          onChange={(value) => {
            onChange(value);
            setOpen(false);
          }}
        />
      )}
    </>
  );
}

function OptionsDialog({
  model,
  target,
  effort,
  disabled,
  onChange,
  onClose,
  allowDefault,
}: Parameters<typeof ModelOptionsButton>[0] & { onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const id = useId();
  const [value, setValue] = useState(effort ?? undefined);
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const modal = dialog.current;
    modal?.showModal();
    return () => modal?.close();
  }, []);
  return createPortal(
    <dialog
      ref={dialog}
      className="panel api-model-picker"
      aria-labelledby={id}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <h2 id={id}>模型选项</h2>
      <p>{model}</p>
      <ReasoningOptions
        target={target}
        model={model}
        value={value}
        disabled={disabled}
        onChange={setValue}
        onReady={setReady}
        allowDefault={allowDefault}
      />
      <div className="api-model-actions">
        <button type="button" className="secondary-button" onClick={onClose}>
          取消
        </button>
        <button
          type="button"
          className="primary"
          disabled={disabled || !ready}
          onClick={() => onChange(value)}
        >
          确认思考强度
        </button>
      </div>
    </dialog>,
    document.body,
  );
}
