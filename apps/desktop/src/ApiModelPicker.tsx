import { useEffect, useId, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { ApiTarget } from "./contracts";
import { apiTargetKey, matchesApiModel, type ApiModelAssignment } from "./apiModels";
import { HelpHeading } from "./HelpHeading";

type Props = {
  target: ApiTarget;
  assignments: ApiModelAssignment[];
  models: string[];
  keyConfigured: boolean;
  ready: boolean;
  checking: boolean;
  disabled: boolean;
  valid: boolean;
  limited?: boolean;
  failed: boolean;
  request: number;
  autoPrompt: boolean;
  onCheck: () => void;
};

let activePicker: string | null = null;

export function ApiModelPicker(props: Props) {
  // A changed endpoint starts a fresh selection and cannot confirm an old list.
  return <ModelPicker key={apiTargetKey(props.target)} {...props}/>;
}

function ModelPicker({ target, assignments, models, keyConfigured, ready, checking, disabled, valid, limited, failed, request, autoPrompt, onCheck }: Props) {
  const root = useRef<HTMLDivElement>(null);
  const dialog = useRef<HTMLDialogElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const prompted = useRef(false);
  const handledRequest = useRef(request);
  const [open, setOpen] = useState(false);
  const [assignmentId, setAssignmentId] = useState(assignments[0]?.id ?? "");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState("");
  const id = useId();
  const targetKey = apiTargetKey(target);
  const currentModel = (assignment: ApiModelAssignment) => apiTargetKey(assignment.target) === targetKey ? assignment.model : "";
  const assignment = assignments.find(item => item.id === assignmentId);
  const catalog = useMemo(() => [...new Set(models)].filter(model => model.trim()), [models]);
  const results = useMemo(() => catalog.filter(model => matchesApiModel(model, query)), [catalog, query]);
  const missing = assignments.some(item => !currentModel(item).trim());

  function show() {
    if (activePicker && activePicker !== id) return;
    const next = assignments.find(item => !currentModel(item).trim()) ?? assignments[0];
    if (!next) return;
    activePicker = id;
    setAssignmentId(next.id);
    setSelected(currentModel(next));
    setQuery("");
    prompted.current = true;
    setOpen(true);
  }

  useEffect(() => {
    if (!keyConfigured) { setOpen(false); prompted.current = false; return; }
    const element = root.current;
    if (!element) return;
    const parents: HTMLDetailsElement[] = [];
    for (let parent = element.parentElement; parent; parent = parent.parentElement) {
      if (parent instanceof HTMLDetailsElement) parents.push(parent);
    }
    const detect = () => {
      if (activePicker === id) { handledRequest.current = request; return; }
      if (disabled || element.closest("[hidden]") || parents.some(parent => !parent.open)
        || !element.getClientRects().length || document.querySelector("dialog.api-model-picker[open]")) return;
      if (request !== handledRequest.current) { handledRequest.current = request; if (!failed) show(); }
      else if (autoPrompt && missing && !prompted.current) show();
    };
    detect();
    parents.forEach(parent => parent.addEventListener("toggle", detect));
    return () => parents.forEach(parent => parent.removeEventListener("toggle", detect));
  }, [keyConfigured, disabled, missing, request, targetKey, failed, autoPrompt]);

  useEffect(() => {
    if (!open || !dialog.current) return;
    const modal = dialog.current;
    modal.showModal();
    search.current?.focus();
    return () => { modal.close(); if (activePicker === id) activePicker = null; };
  }, [open]);
  useEffect(() => () => { if (activePicker === id) activePicker = null; }, [id]);

  if (!assignments.length) return null;
  const canConfirm = !disabled && keyConfigured && ready && valid && !!assignment && catalog.includes(selected);
  return <div className="api-model-selection" ref={root}>
    <div className="api-model-current">{assignments.map(item => <p key={item.id}>
      <strong>{item.label}</strong><span>{currentModel(item) || "待选择具体模型"}</span>
    </p>)}</div>
    <button type="button" className="secondary-button" disabled={disabled || !keyConfigured} onClick={show}>选择或修改模型</button>
    {open && createPortal(<dialog ref={dialog} className="panel api-model-picker" aria-modal="true" aria-labelledby={`${id}-title`}
      onCancel={event => { event.preventDefault(); setOpen(false); }}>
      <HelpHeading as="h2" help="搜索供应商返回的模型目录，确认后将完整模型 ID 写入所选角色。各角色和执行配置分别保存自己的模型。" id={`${id}-title`}>选择具体模型</HelpHeading>
      <p className="secondary">选择 {assignment?.label ?? "角色"} 使用的完整模型名称。</p>
      {assignments.length > 1 && <>
        <label htmlFor={`${id}-assignment`}>应用到</label>
        <select id={`${id}-assignment`} value={assignmentId} disabled={disabled} onChange={event => {
          const next = assignments.find(item => item.id === event.target.value);
          setAssignmentId(event.target.value); setSelected(next ? currentModel(next) : "");
        }}>{assignments.map(item => <option key={item.id} value={item.id}>{item.label}</option>)}</select>
      </>}
      <label htmlFor={`${id}-search`}>搜索模型名称</label>
      <input ref={search} id={`${id}-search`} type="search" value={query} autoComplete="off" maxLength={512}
        placeholder="简称、全称或字母数字，如 ds、DeepSeek、qwen3 32b" onChange={event => setQuery(event.target.value)}/>
      <p className="secondary">支持大小写、空格及标点差异。搜索后请明确选中一项。</p>
      <div className="api-model-catalog-status">
        <button type="button" className="secondary-button" disabled={disabled || !keyConfigured || !valid} onClick={onCheck}>
          {checking ? "正在读取…" : ready ? "刷新模型目录" : "读取模型目录"}
        </button>
        <p role="status">{checking ? "正在读取当前端点的模型目录。" : failed ? "模型目录读取失败，请核对接入配置后重试。" :
          !valid ? "请先填写有效的接入端点和业务空间。" : !ready ? "Key 已保存，请读取当前端点的模型目录后选择。" :
          !catalog.length ? "当前目录为空，请核对账户或端点后重新读取。" : `匹配 ${results.length} / ${catalog.length} 个模型`}</p>
      </div>
      {ready && limited && <p className="secondary">当前返回部分目录，可刷新目录或使用项目中的完整模型 ID 输入框。</p>}
      {ready && <fieldset className="api-model-results" disabled={disabled}>
        <legend>候选模型</legend>
        {results.map(model => <label className="api-model-option" key={model}>
          <input type="radio" name={`${id}-model`} value={model} checked={selected === model} onChange={() => setSelected(model)}/>
          <span>{model}</span>
        </label>)}
        {!!catalog.length && !results.length && <p>没有匹配的模型，请调整搜索词。</p>}
      </fieldset>}
      <p className="api-model-confirmation">已选模型 <strong>{selected || "尚未选择"}</strong></p>
      <div className="api-model-actions">
        <button type="button" className="secondary-button" onClick={() => setOpen(false)}>稍后选择</button>
        <button type="button" className="primary" disabled={!canConfirm} onClick={() => {
          if (!canConfirm || !assignment) return;
          assignment.onSelect(selected, target); setOpen(false);
        }}>确认使用此模型</button>
      </div>
    </dialog>, document.body)}
  </div>;
}
