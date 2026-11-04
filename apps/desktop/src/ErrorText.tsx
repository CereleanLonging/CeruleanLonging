import { useState } from "react";

const translations: [string, string][] = [
  ["Windows 凭证加解密失败", "本机凭证加解密失败"],
  ["Codex 返回的会话标识与所填标识不一致", "返回的 Codex 会话标识与所填标识不一致，请核对目标聊天。"],
  ["linked or invalid host path", "路径类型不正确或使用了链接。请选择实际文件；需要程序路径时，请填写完整的 .exe 路径。"],
  ["reparse host path", "该路径是链接、应用执行别名或重解析点，请选择实际文件路径。"],
  ["Codex process launch failed", "无法启动 Codex。请确认所选文件是 Codex 命令行程序，并检查当前账户的运行权限。"],
  ["Codex check runtime unavailable", "无法启动 Codex 连接检查，请检查本机运行环境。"],
  ["Codex observation timeout; result unknown", "等待 Codex 响应超时，结果尚未确认；请核对原记录，不要重复发送任务。"],
  ["Codex send timeout; delivery unknown", "向 Codex 发送请求超时，是否送达尚未确认；请先核对原记录。"],
  ["Codex write failed; delivery unknown", "与 Codex 的通信写入失败，是否送达尚未确认；请先核对原记录。"],
  ["Codex transport uncertain; reconnect and reconcile", "Codex 通信状态尚未确认，请重新连接并核对原记录。"],
  ["Codex transport uncertain; reconcile", "Codex 通信状态尚未确认，请核对原记录。"],
  ["Codex read failed", "读取 Codex 响应失败，请核对连接和原记录。"],
  ["Codex model list missing", "Codex 未返回可用模型列表，请检查登录状态及版本兼容性。"],
  ["Codex result missing", "Codex 响应缺少结果，请核对原记录和版本兼容性。"],
  ["Codex reply identity mismatch", "Codex 响应与本次请求不匹配，未将其视为成功。"],
  ["Codex process required forced cleanup after close timeout", "Codex 检查进程未能在时限内正常关闭，已执行进程清理；请核对检查结果。"],
  ["Codex process wait failed", "无法确认 Codex 进程是否已退出，请核对原状态。"],
  ["Codex process cleanup failed", "Codex 进程清理未完成，请核对原状态。"],
  ["Access is denied. (os error 5)", "访问被拒绝，当前 Windows 账户没有所需权限（错误 5）。"],
  ["The system cannot find the file specified. (os error 2)", "找不到指定文件，请检查文件是否存在（错误 2）。"],
  ["The system cannot find the path specified. (os error 3)", "找不到指定目录，请检查完整路径（错误 3）。"],
];

export function describeError(error: unknown): { message: string; detail: string | null } {
  const original = error instanceof Error ? error.message : String(error ?? "");
  const raw = original.replace(/^Error:\s*/, "");
  let message = raw;
  for (const [english, chinese] of translations) message = message.replaceAll(english, chinese);
  if (message !== raw) return { message, detail: original };
  if (/^Codex RPC rejected request/.test(raw)) return { message: "Codex 拒绝了本次协议请求，请核对登录状态及版本兼容性。", detail: original };
  if (/^(?:HTTP\s*)?(?:401|403)\b|unauthorized|forbidden/i.test(raw)) return { message: "身份验证或访问权限检查未通过，请核对所选服务、凭证及账户权限。", detail: original };
  if (/^\s*[\u3400-\u9fff]/.test(raw)) return { message: raw, detail: null };
  if (/timeout|timed out/i.test(raw)) return { message: "操作等待超时，结果尚未确认。请先读取原记录核对，再决定后续处理。", detail: original };
  return { message: "操作未完成。请展开技术详情查看原因，并核对当前配置及原记录。", detail: original || null };
}

/** Translate presentation only; original diagnostics and persisted evidence remain intact. */
export function ErrorText({ error }: { error: unknown }) {
  const [expanded, setExpanded] = useState(false);
  const { message, detail } = describeError(error);
  return <span className="error-message">
    <span>{message}</span>
    {detail && <> <button type="button" className="error-detail-toggle" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>{expanded ? "收起技术详情" : "技术详情"}</button>
      {expanded && <code className="error-detail">{detail}</code>}
    </>}
  </span>;
}
