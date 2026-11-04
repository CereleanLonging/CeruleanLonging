import { HelpHeading } from "./HelpHeading";
import type { ProjectWorkerAdapter, SupervisionAdviceView, SupervisionOverride, SupervisionPolicy } from "./contracts";
import { apiWorkerOptions } from "./ApiWorkerFields";

export function canRecordSupervision(adapter: ProjectWorkerAdapter): boolean {
  return adapter === "codex" || adapter === "omp" || apiWorkerOptions.some(([id]) => id === adapter);
}

export function supervisionConflictFor(adapter: ProjectWorkerAdapter, policy: SupervisionPolicy | null | undefined): boolean {
  return !!policy && !canRecordSupervision(adapter);
}

export function supervisionDeliverySummary(advice: SupervisionAdviceView): string {
  if (advice.omp) {
    switch (advice.deliveryState) {
      case "recordOnly": return "OMP 建议仅保存供审阅，未向原任务发送";
      case "awaitingApplication": return "等待原任务应用建议，尚未确认投送";
      case "appliedLocally": return "建议已在本地入账，尚无原任务接收证明";
      case "applicationUnknown": case "adviceOutcomeUnknown":
        return "建议应用或投送结果待核对，不会自动重发";
      case "commandAccepted": return "OMP 已确认建议命令；尚未确认建议入队或出现在原任务消息流";
      case "queued": return "已确认建议进入原任务队列，尚未在原任务消息流中观察到建议";
      case "toolResultReleased": return "原工具结果已在本地刷新到传输通道；尚无对端接收确认或原任务消息流证明";
      case "messageObserved": return "原任务消息流已观察到建议，不代表模型执行或验收";
      case "notSent": return "建议在发送前撤回，原任务继续执行";
      default: return "OMP 建议状态待核对，不会自动重发";
    }
  }
  if (advice.api) {
    switch (advice.deliveryState) {
      case "responseObserved": return "携带建议的下一轮请求已收到回复；不代表已执行建议或通过验收";
      case "nextRequestActive": return "下一轮请求正在进行，尚未确认回复";
      case "applicationUnknown": case "linkPublicationUnknown": case "requestOutcomeUnknown":
        return "建议应用或请求结果待核对，不会自动重发";
      case "appliedLocally": case "awaitingNextRequest": return "建议已在本地应用，尚无下一轮回复证明";
    }
  }
  return advice.delivered ? "原执行轮次已确认接收；不代表已执行或通过验收" : advice.deliveryState === "unknown" ? "建议应用或送达待核对，不会重发" : "尚未送达执行器";
}

export const defaultSupervision: SupervisionPolicy = {
  schemaVersion: 1,
  repeatedFailureThreshold: 2,
  maxAdvicePerAttempt: 0,
};

export function effectiveSupervision(
  override: SupervisionOverride | null | undefined,
  inherited: SupervisionPolicy | null | undefined,
): SupervisionPolicy | undefined {
  if (override?.mode === "disabled") return undefined;
  return override?.mode === "enabled" ? override.policy : inherited ?? undefined;
}

export function supervisionSummary(policy: SupervisionPolicy | null | undefined): string {
  if (!policy) return "未启用异常监督";
  if (policy.maxAdvicePerAttempt === 0)
    return `同一操作连续失败 ${policy.repeatedFailureThreshold} 次记录异常；不请求或发送建议。`;
  return `同一操作连续失败 ${policy.repeatedFailureThreshold} 次记录异常；每次尝试最多 ${policy.maxAdvicePerAttempt} 条建议，计入项目累计请求上限。${policy.deliveryEnabled ? "允许向原执行轮次发送建议；未知回执不重发。" : "可请求并保存建议，不向执行器发送。"}`;
}

export function SupervisionFields({ id, value, inherited, adapter, onChange }: {
  id: string;
  value: SupervisionOverride | null | undefined;
  inherited: SupervisionPolicy | null | undefined;
  adapter: ProjectWorkerAdapter;
  onChange: (next: SupervisionOverride | undefined) => void;
}) {
  const policy = effectiveSupervision(value, inherited);
  const supported = canRecordSupervision(adapter);
  const conflict = supervisionConflictFor(adapter, policy);
  const custom = value?.mode === "enabled" ? value.policy : undefined;
  function update(patch: Partial<SupervisionPolicy>) {
    if (custom) onChange({ mode: "enabled", policy: { ...custom, ...patch } });
  }
  return <section className="panel" aria-label="执行配置异常监督">
    <HelpHeading as="h4" topic="SupervisionFields">此执行配置的异常监督</HelpHeading>
    <label htmlFor={`${id}-mode`}>监督方式</label>
    <select id={`${id}-mode`} value={value?.mode ?? "inherit"} onChange={e => {
      if (e.target.value === "inherit") onChange(undefined);
      else if (e.target.value === "disabled") onChange({ mode: "disabled" });
      else onChange({ mode: "enabled", policy: { ...(inherited ?? defaultSupervision) } });
    }}>
      <option value="inherit">沿用项目默认</option>
      <option value="disabled">关闭此配置的监督</option>
      <option value="enabled" disabled={!supported}>单独设置监督</option>
    </select>
    {custom && supported && <>
      <div className="planning-fields">
        <div><label htmlFor={`${id}-threshold`}>{adapter === "omp" ? "同一文件工具连续失败阈值" : "同一命令连续失败阈值"}</label><input id={`${id}-threshold`} type="number" min={2} max={8} required value={custom.repeatedFailureThreshold} onChange={e => update({ repeatedFailureThreshold: Number(e.target.value) })}/></div>
        <div><label htmlFor={`${id}-limit`}>每次尝试建议上限</label><input id={`${id}-limit`} type="number" min={0} max={2} required value={custom.maxAdvicePerAttempt} onChange={e => update({ maxAdvicePerAttempt: Number(e.target.value), ...(Number(e.target.value) === 0 ? { deliveryEnabled: false } : {}) })}/></div>
      </div>
      <label className="checkbox-label"><input type="checkbox" checked={!!custom.deliveryEnabled} disabled={!custom.deliveryEnabled && custom.maxAdvicePerAttempt === 0} onChange={e => update({ deliveryEnabled: e.target.checked })}/>允许向此配置的当前执行轮次发送建议</label>
    </>}
    <p className="secondary">{supervisionSummary(policy)}</p>
    {!supported && <p className="secondary">此接入尚不支持运行中异常监督；可关闭此配置的监督，与启用监督的 Codex 一起使用。</p>}
    {adapter === "omp" && <p className="secondary">OMP 根据原文件工具的连续失败请求副指挥建议，计入管理请求额度。允许投送时，在原文件工具结果等待期间发送一次建议，并核对原任务队列与消息流；结果未知不重发。未允许投送时只保存建议供审阅。</p>}
    {supported && adapter !== "codex" && adapter !== "omp" && <p className="secondary">允许投送时，API 在工具执行后等待建议，并将建议计入下一轮请求。未允许投送时只保存建议供审阅。</p>}
    {conflict && <p role="alert">此执行接入不能使用当前监督策略。请关闭此配置的监督，或单独设置支持的策略。</p>}
  </section>;
}
