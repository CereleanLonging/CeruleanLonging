import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Command, Snapshot, ReasoningEffort } from "./contracts";
import { reasoningLabel } from "./ReasoningOptions";
import { ProviderCard, providers } from "./ProviderConnections";
import { LocalConnectionTest } from "./LocalConnectionTest";
import { preferenceKey, readPreference } from "./accessPreferences";

type Props = { data: Snapshot; busy: boolean; send: (command: Command) => Promise<boolean> };

export function ompPreference(scope: string, role: "deputy" | "worker") {
  const prefix = role === "deputy" ? "deputy-" : "";
  const read = (field: string) => readPreference(preferenceKey(scope, `${prefix}omp-test-${field}`));
  const savedEffort = read("effort");
  const effort: ReasoningEffort = savedEffort && ["none", "low", "high", "max"].includes(savedEffort)
    ? savedEffort as ReasoningEffort : "max";
  return {executable:read("executable")??"", model:read("model")||"deepseek-flash", effort, runtimeParent:read("runtime")||undefined};
}

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

export function OmpConnectionTest({data,busy,send,executable,model,runtimeParent,effort,onMissing}: Props & {executable:string;model:string;runtimeParent?:string|null;effort?:ReasoningEffort|null;onMissing?:()=>void}) {
  const [prompt,setPrompt] = useState(false);
  const [confirm,setConfirm] = useState(false);
  const decision = useRef<((value:boolean)=>void) | null>(null);
  useEffect(() => {
    decision.current?.(false);
    decision.current = null;
    setConfirm(false);
    return () => { decision.current?.(false); decision.current = null; };
  }, [executable, model, runtimeParent, effort, data.dataDirectory]);
  function finish(value:boolean) { decision.current?.(value); decision.current=null; setConfirm(false); }
  return <>
    <LocalConnectionTest scope={data.dataDirectory} busy={busy}
      request={executable.trim() && model.trim() ? {kind:"omp",executable:executable.trim(),model:model.trim(),runtimeParent,reasoningEffort:effort} : null}
      beforeTest={() => {if (!hasOmpKey(data)) {setPrompt(true); return false;} setConfirm(true); return new Promise<boolean>(resolve => {decision.current=resolve;});}}
      onError={error => {if (error.includes("未返回所选模型") || error.includes("provider/model not supported") || error.includes("尚未确认支持")) { if (onMissing) onMissing(); else setPrompt(true); }}}
      hint="OMP 使用独立的本地 Agent 程序，通过 DeepSeek API 调用模型。连接检查使用工作台已保存的 Key；缺少时会提示补录。"/>
    {!hasOmpKey(data) && <button type="button" className="secondary-button" disabled={busy} onClick={() => setPrompt(true)}>补充 OMP API Key</button>}
    {prompt && <OmpKeyDialog data={data} busy={busy} send={send} onClose={() => setPrompt(false)}/>}
    {confirm && <OmpConfirmation model={model} effort={effort} onDecision={finish}/>}
  </>;
}

function OmpConfirmation({model,effort,onDecision}:{model:string;effort?:ReasoningEffort|null;onDecision:(value:boolean)=>void}) {
  const dialog=useRef<HTMLDialogElement>(null);
  const id=useId();
  useEffect(()=>{const element=dialog.current;element?.showModal();return()=>element?.close();},[]);
  return createPortal(<dialog ref={dialog} className="panel api-model-picker" aria-labelledby={id} onCancel={event=>{event.preventDefault();onDecision(false);}}>
    <h2 id={id}>确认本次 OMP 接入</h2>
    <p>服务为 DeepSeek 官方 API，模型为 <strong>{model}</strong>，思考强度为 {effort?reasoningLabel(effort):"关闭（沿用原配置）"}。</p>
    <p className="secondary">启动隔离的 OMP 程序并核对实际模型，再查询该 Key 的模型目录。</p>
    <div className="api-model-actions"><button type="button" className="secondary-button" onClick={()=>onDecision(false)}>取消</button><button type="button" className="primary" onClick={()=>onDecision(true)}>确认并检查</button></div>
  </dialog>,document.body);
}
