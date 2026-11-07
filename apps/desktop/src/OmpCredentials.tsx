import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Command, Snapshot } from "./contracts";
import { ProviderCard, providers } from "./ProviderConnections";
import { LocalConnectionTest } from "./LocalConnectionTest";

type Props = { data: Snapshot; busy: boolean; send: (command: Command) => Promise<boolean> };

export function hasOmpKey(data: Snapshot): boolean {
  const connection = data.providerConnections?.find(item => item.provider === "deepseek");
  return connection ? connection.keyConfigured : data.providerCredentials?.some(item => item.provider === "deepseek" && item.keyConfigured) === true;
}

export function OmpKeyDialog({data,busy,send,onClose}: Props & {onClose: () => void}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const id = useId();
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  return createPortal(<dialog ref={dialog} className="panel api-model-picker" aria-labelledby={`${id}-title`}
    onCancel={event => {event.preventDefault(); if (!busy) onClose();}}>
    <h2 id={`${id}-title`}>补充 OMP API Key</h2>
    <p>OMP 通过 DeepSeek 官方 API 调用模型，需要本工作台已加密保存的 DeepSeek Key。保存后返回配置，再由你发起连接检查。</p>
    <ProviderCard entry={providers[0]} scope={data.dataDirectory} fixedTarget={{kind:"deepSeek"}}
      status={data.providerConnections?.find(item => item.provider === "deepseek")} busy={busy} send={send} connected view="key" idPrefix={`omp-key-${id}`}/>
    <div className="api-model-actions"><button type="button" className="secondary-button" disabled={busy} onClick={onClose}>返回配置</button></div>
  </dialog>, document.body);
}

export function OmpConnectionTest({data,busy,send,executable,model,runtimeParent}: Props & {executable:string;model:string;runtimeParent?:string|null}) {
  const [prompt,setPrompt] = useState(false);
  return <>
    <LocalConnectionTest scope={data.dataDirectory} busy={busy}
      request={executable.trim() && model.trim() ? {kind:"omp",executable:executable.trim(),model:model.trim(),runtimeParent} : null}
      beforeTest={() => {if (hasOmpKey(data)) return true; setPrompt(true); return false;}}
      hint="OMP 使用独立的本地 Agent 程序，通过 DeepSeek API 调用模型。连接检查使用工作台已保存的 Key；缺少时会提示补录。"/>
    {!hasOmpKey(data) && <button type="button" className="secondary-button" disabled={busy} onClick={() => setPrompt(true)}>补充 OMP API Key</button>}
    {prompt && <OmpKeyDialog data={data} busy={busy} send={send} onClose={() => setPrompt(false)}/>}
  </>;
}
