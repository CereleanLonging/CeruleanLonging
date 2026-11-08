import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import type { ReasoningEffort } from "./contracts";
import { ReasoningOptions, reasoningLabel } from "./ReasoningOptions";

function highestEffort(options: ReasoningEffort[]) {
  return (
    [
      "ultra",
      "max",
      "xhigh",
      "high",
      "medium",
      "low",
      "minimal",
      "none",
    ] as ReasoningEffort[]
  ).find((value) => options.includes(value));
}

type Model = { model: string; supportedReasoningEfforts: ReasoningEffort[] };
type Props = {
  executable: string;
  model: string;
  effort?: ReasoningEffort | null;
  busy: boolean;
  onSelect: (model: string, effort?: ReasoningEffort) => void;
  onMissing: () => void;
};

export function CliModelPicker(props: Props) {
  const [open, setOpen] = useState(false);
  return (
    <div className="cli-model-selection">
      <p>
        {props.model || "待选择 CLI 模型"} · 思考 {reasoningLabel(props.effort)}
      </p>
      <button
        type="button"
        className="secondary-button"
        disabled={props.busy || !props.executable.trim()}
        onClick={() => setOpen(true)}
      >
        选择 CLI 模型
      </button>
      {open && (
        <CliDialog
          key={props.executable}
          {...props}
          onClose={() => setOpen(false)}
        />
      )}
    </div>
  );
}

function CliDialog({
  executable,
  model,
  effort,
  busy,
  onSelect,
  onMissing,
  onClose,
}: Props & { onClose: () => void }) {
  const modal = useRef<HTMLDialogElement>(null);
  const id = useId();
  const [models, setModels] = useState<Model[] | null>(null);
  const [error, setError] = useState("");
  const [selected, setSelected] = useState(model);
  const [value, setValue] = useState(effort ?? undefined);
  const [step, setStep] = useState(false);
  const [ready, setReady] = useState(false);
  const chosen = models?.find((item) => item.model === selected);
  useEffect(() => {
    let active = true;
    const dialog = modal.current;
    dialog?.showModal();
    void invoke<Model[]>("cli_model_catalog", { executable })
      .then((items) => {
        if (!active) return;
        if (
          !items.length ||
          (model.trim() && !items.some((item) => item.model === model))
        ) {
          onClose();
          onMissing();
          return;
        }
        setModels(items);
        if (!effort)
          setValue(
            highestEffort(
              items.find((item) => item.model === model)
                ?.supportedReasoningEfforts ?? [],
            ),
          );
      })
      .catch(() => {
        if (active)
          setError(
            "无法读取当前 CLI 登录的模型目录，请检查程序与登录状态，或配置模型 Key 接入。",
          );
      });
    return () => {
      active = false;
      dialog?.close();
    };
  }, [executable]);
  return createPortal(
    <dialog
      ref={modal}
      className="panel api-model-picker"
      aria-labelledby={id}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
    >
      <h2 id={id}>{step ? "CLI 思考强度" : "选择 CLI 模型"}</h2>
      {step && chosen ? (
        <ReasoningOptions
          target={{ kind: "openAi" }}
          model={selected}
          value={value}
          options={chosen.supportedReasoningEfforts}
          onChange={setValue}
          onReady={setReady}
          disabled={busy}
        />
      ) : (
        <>
          {error ? (
            <p role="alert">{error}</p>
          ) : !models ? (
            <p role="status">正在读取当前 CLI 登录支持的模型…</p>
          ) : (
            <fieldset className="api-model-results">
              <legend>当前可用模型</legend>
              {models.map((item) => (
                <label className="api-model-option" key={item.model}>
                  <input
                    type="radio"
                    checked={item.model === selected}
                    onChange={() => {
                      setSelected(item.model);
                      setValue(highestEffort(item.supportedReasoningEfforts));
                    }}
                  />
                  {item.model}
                </label>
              ))}
            </fieldset>
          )}
          <button
            type="button"
            className="secondary-button"
            onClick={() => {
              onClose();
              onMissing();
            }}
          >
            所需模型不在目录中 · 配置对应 Key
          </button>
        </>
      )}
      <div className="api-model-actions">
        <button
          type="button"
          className="secondary-button"
          onClick={() => (step ? setStep(false) : onClose())}
        >
          {step ? "返回选模" : "取消"}
        </button>
        <button
          type="button"
          className="primary"
          disabled={busy || !chosen || (step && !ready)}
          onClick={() => {
            if (!chosen) return;
            if (!step) {
              setStep(true);
              return;
            }
            onSelect(selected, value);
            onClose();
          }}
        >
          {step ? "确认模型与思考强度" : "下一步 · 思考强度"}
        </button>
      </div>
    </dialog>,
    document.body,
  );
}
