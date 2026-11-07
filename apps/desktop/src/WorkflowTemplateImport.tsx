import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useImperativeHandle, useRef, useState, type Ref } from "react";
import type { SupervisionPolicy, WorkflowTemplateReview } from "./contracts";
import example from "./workflow-template-example.json";
import { ImportReviewDialog } from "./ImportReviewDialog";

const reviewLocal = (input: string) => invoke<WorkflowTemplateReview>("review_workflow_template", { input });

export function WorkflowTemplateImport({ contextKey, disabled, onApply, reviewTemplate = reviewLocal, compact = false, fileImportRef }: {
  contextKey: string;
  disabled: boolean;
  compact?: boolean;
  fileImportRef?: Ref<(file: File) => void>;
  onApply: (requirement: string, supervision?: SupervisionPolicy) => void;
  reviewTemplate?: (input: string) => Promise<WorkflowTemplateReview>;
}) {
  const [review, setReview] = useState<WorkflowTemplateReview>();
  const [error, setError] = useState<string>();
  const [reading, setReading] = useState(false);
  const [applySupervision, setApplySupervision] = useState(false);
  const [open, setOpen] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const sequence = useRef(0);
  const current = useRef({ contextKey, disabled });
  current.current = { contextKey, disabled };
  useEffect(() => {
    sequence.current++; setReview(undefined); setError(undefined); setReading(false); setApplySupervision(false);
    setOpen(false);
    return () => { sequence.current++; };
  }, [contextKey, disabled]);

  async function read(file: File | undefined) {
    const attempt = ++sequence.current;
    const selectedContext = contextKey;
    setReview(undefined); setError(undefined); setApplySupervision(false);
    if (!file || disabled) return;
    setOpen(true);
    setReading(true);
    const active = () => attempt === sequence.current && current.current.contextKey === selectedContext && !current.current.disabled;
    try {
      if (file.size > 64 * 1024) throw new Error("工作流模板不得超过64 KiB。");
      const input = await file.text();
      if (!active()) return;
      const result = await reviewTemplate(input);
      if (active()) setReview(result);
    } catch (e) {
      if (active()) setError(String(e));
    } finally {
      if (active()) setReading(false);
    }
  }

  useImperativeHandle(fileImportRef, () => file => { void read(file); });
  function cancel() { sequence.current++; setOpen(false); setReview(undefined); setError(undefined); setReading(false); setApplySupervision(false); }
  const input = <input ref={fileInput} id="workflow-template-file" type="file" accept=".json,application/json" hidden={compact}
    aria-label="本地工作流文件" disabled={disabled || reading}
    onChange={e => { const file = e.target.files?.[0]; e.target.value = ""; void read(file); }} />;
  const content = <>
    <p className="secondary">选择工作流 JSON，预览步骤、完成标准及副指挥监督要点，再填入本项目的需求草稿。模型、工具、产物和验收方式继续使用当前配置。</p>
    <details><summary>模板格式与示例</summary><p>模板步骤用于规划，实际任务安排以最终批准的方案为准。</p><pre>{JSON.stringify(example, null, 2)}</pre></details>
    {!compact && <><label htmlFor="workflow-template-file">本地工作流文件</label>{input}</>}
    {reading && <p role="status">正在读取并检查工作流…</p>}
    {error && <p role="alert"><ErrorText error={error}/></p>}
    {review && <div aria-label="工作流导入预览">
      <HelpHeading as="h4" topic="WorkflowTemplateImport">{review.template.name}</HelpHeading>
      <p>类型：{review.template.workflowType} · {review.template.steps.length} 个步骤</p>
      <label htmlFor="workflow-template-preview">将填入的需求</label>
      <textarea id="workflow-template-preview" rows={10} readOnly value={review.requirement} />
      <p className="secondary">确认后将替换当前需求草稿。你可以继续编辑；保存、请求规划、批准和启动仍需分别操作。</p>
      {review.template.supervision && <label className="checkbox-label"><input type="checkbox" checked={applySupervision} onChange={e=>setApplySupervision(e.target.checked)}/>
        同时应用异常监督设置：连续失败 {review.template.supervision.repeatedFailureThreshold} 次，每次尝试最多 {review.template.supervision.maxAdvicePerAttempt} 条副指挥建议。建议先保存供审阅，投送须在配置中另行启用。
      </label>}
      <button type="button" className="secondary-button" disabled={disabled || reading} onClick={() => {
        if (!disabled && !reading && current.current.contextKey === contextKey) {
          const policy = review.template.supervision;
          onApply(review.requirement, applySupervision && policy ? {schemaVersion:1,repeatedFailureThreshold:policy.repeatedFailureThreshold,maxAdvicePerAttempt:policy.maxAdvicePerAttempt,deliveryEnabled:false} : undefined); setReview(undefined); setOpen(false);
        }
      }}>使用此工作流需求</button>
      <button type="button" className="secondary-button" onClick={cancel}>取消导入</button>
    </div>}
  </>;
  if (compact) return <div className="requirement-import-action workflow-import-compact">
    <button type="button" className="requirement-import-button" aria-label="导入本地工作流" title="导入本地工作流（JSON）"
      disabled={disabled || reading} onClick={() => fileInput.current?.click()}>
      <svg viewBox="0 0 24 24" width="21" height="21" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"/></svg>
    </button>
    {input}<ImportReviewDialog open={open} title="导入本地工作流" onClose={cancel}>{content}</ImportReviewDialog>
  </div>;
  return <section className="panel" aria-label="导入本地工作流"><HelpHeading as="h3" topic="WorkflowTemplateImport">导入本地工作流</HelpHeading>{content}</section>;
}
