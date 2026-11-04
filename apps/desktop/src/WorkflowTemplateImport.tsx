import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import type { SupervisionPolicy, WorkflowTemplateReview } from "./contracts";
import example from "./workflow-template-example.json";

const reviewLocal = (input: string) => invoke<WorkflowTemplateReview>("review_workflow_template", { input });

export function WorkflowTemplateImport({ contextKey, disabled, onApply, reviewTemplate = reviewLocal }: {
  contextKey: string;
  disabled: boolean;
  onApply: (requirement: string, supervision?: SupervisionPolicy) => void;
  reviewTemplate?: (input: string) => Promise<WorkflowTemplateReview>;
}) {
  const [review, setReview] = useState<WorkflowTemplateReview>();
  const [error, setError] = useState<string>();
  const [reading, setReading] = useState(false);
  const [applySupervision, setApplySupervision] = useState(false);
  const sequence = useRef(0);
  const current = useRef({ contextKey, disabled });
  current.current = { contextKey, disabled };
  useEffect(() => {
    sequence.current++; setReview(undefined); setError(undefined); setReading(false); setApplySupervision(false);
    return () => { sequence.current++; };
  }, [contextKey, disabled]);

  async function read(file: File | undefined) {
    const attempt = ++sequence.current;
    const selectedContext = contextKey;
    setReview(undefined); setError(undefined); setApplySupervision(false);
    if (!file || disabled) return;
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

  return <section className="panel" aria-label="导入本地工作流">
    <HelpHeading as="h3" topic="WorkflowTemplateImport">导入本地工作流</HelpHeading>
    <p className="secondary">选择工作流 JSON，预览步骤、完成标准及副指挥监督要点，再填入本项目的需求草稿。模型、工具、产物和验收方式继续使用当前配置。</p>
    <details><summary>模板格式与示例</summary><p>模板步骤用于规划，实际任务安排以最终批准的方案为准。</p><pre>{JSON.stringify(example, null, 2)}</pre></details>
    <label htmlFor="workflow-template-file">本地工作流文件</label>
    <input id="workflow-template-file" type="file" accept=".json,application/json" disabled={disabled || reading}
      onChange={e => { const file = e.target.files?.[0]; e.target.value = ""; void read(file); }} />
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
          onApply(review.requirement, applySupervision && policy ? {schemaVersion:1,repeatedFailureThreshold:policy.repeatedFailureThreshold,maxAdvicePerAttempt:policy.maxAdvicePerAttempt,deliveryEnabled:false} : undefined); setReview(undefined);
        }
      }}>使用此工作流需求</button>
      <button type="button" className="secondary-button" onClick={() => setReview(undefined)}>取消导入</button>
    </div>}
  </section>;
}
