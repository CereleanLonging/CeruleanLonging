import { useRef } from "react";
import workflowImage from "../../../docs/images/workflow-0.1.48-zh.svg";
import "./WorkflowMap.css";

const description = "完整流程从项目接入、主指挥问答、用户批准开始，经副指挥分配、执行端提交、独立逐任务检查、主指挥接受与核心汇总，再进入独立盲评、主指挥功能测试和用户最终确认。图中包含返工、方案修订、目录隔离与中断处理。";

export function WorkflowMap() {
  const dialog = useRef<HTMLDialogElement>(null);
  return (
    <section className="panel workflow-map" aria-labelledby="workflow-map-heading">
      <div className="section-heading">
        <h2 id="workflow-map-heading">完整工作流程</h2>
        <button type="button" className="secondary-button" onClick={() => dialog.current?.showModal()}>放大查看</button>
      </div>
      <div className="workflow-map-viewport" tabIndex={0} role="region" aria-label="完整流程图，可滚动查看">
        <img src={workflowImage} alt={description} width={1180} height={1756} />
      </div>
      <dialog ref={dialog} className="workflow-map-dialog" aria-labelledby="workflow-map-dialog-heading">
        <div className="section-heading">
          <h2 id="workflow-map-dialog-heading">完整工作流程</h2>
          <button type="button" className="secondary-button" autoFocus onClick={() => dialog.current?.close()}>关闭流程图</button>
        </div>
        <div className="workflow-map-viewport" tabIndex={0} role="region" aria-label="放大的完整流程图，可滚动查看">
          <img src={workflowImage} alt={description} width={1180} height={1756} />
        </div>
      </dialog>
    </section>
  );
}
