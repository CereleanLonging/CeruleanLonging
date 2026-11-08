import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type {
  ApiTarget,
  Command,
  ReasoningEffort,
  Snapshot,
} from "./contracts";
import { ProviderCard, providers } from "./ProviderConnections";
import { OpenAiConnection } from "./OpenAiConnection";
import { revealConnectionField } from "./ConnectionMethods";

export function ModelCredentialDialog({
  data,
  busy,
  send,
  model,
  onClose,
  onSelect,
  focusTarget,
}: {
  data: Snapshot;
  busy: boolean;
  send: (command: Command) => Promise<boolean>;
  model: string;
  onClose: () => void;
  onSelect: (
    model: string,
    target: ApiTarget,
    effort?: ReasoningEffort,
  ) => void;
  focusTarget?: string;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const id = useId();
  const [provider, setProvider] = useState("openai");
  useEffect(() => {
    const modal = dialog.current;
    modal?.showModal();
    return () => modal?.close();
  }, []);
  const entry = providers.find((item) => item.id === provider);
  const assignments = [
    {
      id: "fallback",
      label: "替换当前角色接入",
      model: "",
      target: { kind: "openAi" } as ApiTarget,
      onSelect: (
        selected: string,
        target: ApiTarget,
        effort?: ReasoningEffort,
      ) => {
        onSelect(selected, target, effort);
        onClose();
        requestAnimationFrame(() => {
          const field = focusTarget
            ? document.getElementById(focusTarget)
            : null;
          if (field) revealConnectionField(field);
        });
      },
    },
  ];
  return createPortal(
    <dialog
      ref={dialog}
      className="panel api-model-picker"
      aria-labelledby={id}
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
    >
      <h2 id={id}>补充所需模型的 Key</h2>
      <p>
        请确认 {model || "所需模型"} 所属的服务，填写对应
        Key。读取该服务模型目录并确认后，将当前角色接入切换为 Key 直连。
      </p>
      <label htmlFor={`${id}-provider`}>Key 所属服务</label>
      <select
        id={`${id}-provider`}
        value={provider}
        disabled={busy}
        onChange={(event) => setProvider(event.target.value)}
      >
        <option value="openai">OpenAI</option>
        {providers.map((item) => (
          <option key={item.id} value={item.id}>
            {item.name}
          </option>
        ))}
      </select>
      {entry ? (
        <ProviderCard
          key={provider}
          entry={entry}
          scope={data.dataDirectory}
          status={data.providerConnections?.find(
            (item) => item.provider === entry.id,
          )}
          busy={busy}
          send={send}
          connected
          view="key"
          idPrefix={`${id}-key`}
          modelAssignments={assignments}
        />
      ) : (
        <OpenAiConnection
          status={data.openaiConnection}
          busy={busy}
          send={send}
          connected
          view="key"
          idPrefix={`${id}-key`}
          modelAssignments={assignments}
        />
      )}
      <div className="api-model-actions">
        <button
          type="button"
          className="secondary-button"
          disabled={busy}
          onClick={onClose}
        >
          返回原配置
        </button>
      </div>
    </dialog>,
    document.body,
  );
}
