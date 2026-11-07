import { useEffect, useRef, useState, type DragEvent } from "react";
import { MarkdownImport } from "./MarkdownImport";
import { WorkflowTemplateImport } from "./WorkflowTemplateImport";
import { FileImportWindow } from "./FileImportWindow";
import type { SupervisionPolicy } from "./contracts";

export function RequirementComposer({ contextKey, disabled, value, onChange, onWorkflow }: {
  contextKey: string;
  disabled: boolean;
  value: string;
  onChange: (text: string) => void;
  onWorkflow: (text: string, supervision?: SupervisionPolicy) => void;
}) {
  const markdownImport = useRef<(file: File) => void>(null);
  const workflowImport = useRef<(file: File) => void>(null);
  const dragDepth = useRef(0);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState("");
  const [nativeBusy, setNativeBusy] = useState(false);
  const [pendingFile, setPendingFile] = useState<File>();

  useEffect(() => {
    if (!pendingFile || nativeBusy) return;
    if (!disabled) {
      if (/\.md$/i.test(pendingFile.name)) markdownImport.current?.(pendingFile);
      else workflowImport.current?.(pendingFile);
    }
    setPendingFile(undefined);
  }, [pendingFile, nativeBusy, disabled]);

  function resetDrag() {
    dragDepth.current = 0;
    setDragging(false);
  }

  useEffect(() => {
    resetDrag();
    setError("");
    setPendingFile(undefined);
  }, [contextKey, disabled]);

  useEffect(() => {
    // File drops outside this field must not navigate away from the draft.
    const preventFileNavigation = (event: globalThis.DragEvent) => {
      if (event.dataTransfer?.types.includes("Files")) event.preventDefault();
    };
    const finishDrag = () => resetDrag();
    window.addEventListener("dragover", preventFileNavigation);
    window.addEventListener("drop", preventFileNavigation);
    window.addEventListener("drop", finishDrag);
    window.addEventListener("dragend", finishDrag);
    window.addEventListener("blur", finishDrag);
    return () => {
      window.removeEventListener("dragover", preventFileNavigation);
      window.removeEventListener("drop", preventFileNavigation);
      window.removeEventListener("drop", finishDrag);
      window.removeEventListener("dragend", finishDrag);
      window.removeEventListener("blur", finishDrag);
    };
  }, []);

  function enter(event: DragEvent<HTMLDivElement>) {
    if (!event.dataTransfer.types.includes("Files")) return;
    event.preventDefault();
    if (disabled || nativeBusy) return;
    dragDepth.current++;
    setDragging(true);
  }

  function leave(event: DragEvent<HTMLDivElement>) {
    if (!event.dataTransfer.types.includes("Files")) return;
    dragDepth.current = Math.max(0, dragDepth.current - 1);
    if (dragDepth.current === 0) setDragging(false);
  }

  function drop(event: DragEvent<HTMLDivElement>) {
    if (!event.dataTransfer.types.includes("Files")) return;
    event.preventDefault();
    resetDrag();
    if (disabled || nativeBusy || document.querySelector("dialog[open]")) return;
    setError("");
    const directories = Array.from(event.dataTransfer.items).some(item => item.webkitGetAsEntry?.()?.isDirectory);
    const files = Array.from(event.dataTransfer.files);
    if (directories || files.length !== 1) {
      setError("请一次拖入一个 .md 文档或工作流 .json 文件。");
      return;
    }
    const file = files[0];
    if (/\.md$/i.test(file.name)) markdownImport.current?.(file);
    else if (/\.json$/i.test(file.name)) workflowImport.current?.(file);
    else setError("仅支持 .md 文档和工作流 .json 文件。");
  }

  return <>
    <div className={`requirement-composer${dragging ? " is-dragging" : ""}`}
      onDragEnter={enter} onDragLeave={leave} onDrop={drop}
      onDragOver={event => {
        if (!event.dataTransfer.types.includes("Files")) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = disabled || nativeBusy ? "none" : "copy";
      }}>
      <textarea id="planning-requirement" required rows={6} disabled={disabled} value={value}
        onChange={event => onChange(event.target.value)}
        placeholder="描述要解决的问题、预期行为和完成标准。需要拖入文件时，请打开下方导入窗口。" />
      <div className="requirement-composer-toolbar" role="group" aria-label="需求附件导入">
        <WorkflowTemplateImport compact fileImportRef={workflowImport} contextKey={contextKey} disabled={disabled || nativeBusy} onApply={onWorkflow} />
        <MarkdownImport compact fileImportRef={markdownImport} mode="requirement" contextKey={contextKey} disabled={disabled || nativeBusy} onApply={onChange} />
        <FileImportWindow contextKey={contextKey} disabled={disabled} onFile={setPendingFile} onBusy={setNativeBusy} />
        <span className="requirement-import-hint">工作流 JSON · Markdown 本地提炼</span>
      </div>
      {dragging && <div className="requirement-drop-hint" role="status">松开文件，预览导入内容</div>}
    </div>
    {error && <p className="requirement-import-error" role="alert">{error}</p>}
  </>;
}
