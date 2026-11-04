import { HelpHeading } from "./HelpHeading";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { WorkspaceReader } from "./WorkspaceBrowser";
import type { WorkspaceResult } from "./contracts";
import { WorkspaceApiPublicationReviews } from "./WorkspaceApiPublicationReviews";
import { WorkspaceManagementRecordReviews } from "./WorkspaceManagementRecordReviews";

const readLogs: WorkspaceReader = query => invoke<WorkspaceResult>("review_logs_read", { query });

export function ReviewLogs({ read = readLogs, enabled = "__TAURI_INTERNALS__" in window }: { read?: WorkspaceReader; enabled?: boolean }) {
  const [tab, setTab] = useState<"api" | "management">("api");
  return <main className="review-logs-window">
    <header><div><div className="eyebrow">LOCAL REVIEW LOGS</div><HelpHeading as="h1" topic="ReviewLogs">只读日志</HelpHeading></div><span className="pill">只读</span></header>
    <p>查看当前工作台的原复核记录。关闭此窗口不影响任务；退出所属工作台时，此窗口会一同关闭。</p>
    {!enabled && <p role="status">请在桌面工作台的“维护与升级”中打开日志窗口。</p>}
    <div className="review-log-tabs" role="tablist" aria-label="日志类型" onKeyDown={event => {
      if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
      event.preventDefault();
      const next = event.key === "Home" ? "api" : event.key === "End" ? "management" : tab === "api" ? "management" : "api";
      setTab(next);document.getElementById(`${next}-log-tab`)?.focus();
    }}>
      <button id="api-log-tab" role="tab" tabIndex={tab === "api" ? 0 : -1} aria-selected={tab === "api"} aria-controls="review-log-content" className={tab === "api" ? "primary" : "secondary-button"} onClick={() => setTab("api")}>API 中断复核记录</button>
      <button id="management-log-tab" role="tab" tabIndex={tab === "management" ? 0 : -1} aria-selected={tab === "management"} aria-controls="review-log-content" className={tab === "management" ? "primary" : "secondary-button"} onClick={() => setTab("management")}>管理复核记录</button>
    </div>
    <div id="review-log-content" role="tabpanel" aria-labelledby={tab === "api" ? "api-log-tab" : "management-log-tab"}>
      {tab === "api" ? <WorkspaceApiPublicationReviews read={read} enabled={enabled} busy={false}/> : <WorkspaceManagementRecordReviews read={read} enabled={enabled} busy={false}/>}
    </div>
  </main>;
}
