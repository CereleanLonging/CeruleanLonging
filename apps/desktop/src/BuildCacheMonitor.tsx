import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ErrorText } from "./ErrorText";
import { HelpHeading } from "./HelpHeading";

type Operation = {
  id: string; at: string; kind: string; completed: boolean;
  bytesBefore: number; bytesRemoved: number; filesRemoved: number;
  skippedActive: number; errors: string[];
};
type Snapshot = {
  projectRoot: string; temporaryRoot: string; logDirectory: string;
  totalBytes: number; availableBytes: number; cumulativeCleanedBytes: number;
  scanCount: number; pendingOperations: number; warnings: string[];
  entries: { path: string; kind: string; bytes: number; files: number; active: boolean }[];
  recent: Operation[];
};
const bytes = (value: number) => value >= 1e9 ? `${(value / 1e9).toFixed(2)} GB`
  : value >= 1e6 ? `${(value / 1e6).toFixed(2)} MB` : `${value.toLocaleString()} 字节`;

export function BuildCacheMonitor({ desktop }: { desktop: boolean }) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [root, setRoot] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  const lock = useRef(false);
  const mounted = useRef(true);
  const rootRef = useRef(root);
  rootRef.current = root;

  async function run(action: "status" | "scan" | "clean", quiet = false) {
    if (!desktop || lock.current) return;
    lock.current = true;
    const requestedRoot = rootRef.current;
    setBusy(true);
    if (!quiet) { setError(""); setMessage(""); }
    try {
      const result = await invoke<Snapshot>("build_cache_monitor", {
        action, projectRoot: rootRef.current.trim() || null,
      });
      if (!mounted.current || requestedRoot !== rootRef.current) return;
      setError("");
      setSnapshot(result);
      if (!rootRef.current) setRoot(result.projectRoot);
      if (action === "scan") setMessage("扫描体积已记录到提炼日志。");
      if (action === "clean") {
        const latest = result.recent.find(item => item.kind === "手动清理");
        setMessage(latest ? `已清理 ${bytes(latest.bytesRemoved)}，保留空目录。${latest.errors.length ? "部分缓存待处理，请查看日志。" : ""}` : "清理结果已记录，请核对当前缓存量。");
      }
    } catch (error) {
      if (mounted.current) setError(String(error));
    } finally {
      lock.current = false;
      if (mounted.current) setBusy(false);
    }
  }

  useEffect(() => {
    mounted.current = true;
    void run("status", true);
    const timer = desktop ? window.setInterval(() => void run("status", true), 30000) : undefined;
    return () => { mounted.current = false; window.clearInterval(timer); };
  }, [desktop]);

  return <section className="panel build-cache-monitor" aria-label="累计缓存监控">
    <HelpHeading as="h2" topic="Maintenance">累计缓存监控</HelpHeading>
    <p>管理工作台源码工程的编译与构建临时缓存。构建、测试结束后自动清理，空目录保留。</p>
    <label htmlFor="build-cache-source-root">工作台源码目录</label>
    <input id="build-cache-source-root" value={root} disabled={!desktop || busy}
      placeholder="留空时定位当前工作台源码工程"
      onChange={event => { setRoot(event.target.value); setSnapshot(null); setMessage(""); setError(""); }} />
    <div className="build-cache-actions">
      <button className="secondary-button" disabled={!desktop || busy} onClick={() => void run("scan")}>扫描全项目临时缓存</button>
      <button className="secondary-button" disabled={!desktop || busy || !snapshot || snapshot.availableBytes === 0}
        onClick={() => void run("clean")}>清理剩余缓存</button>
      {busy && <span role="status">正在核对缓存…</span>}
    </div>
    {!desktop && <p className="secondary">请在桌面工作台中使用缓存监控。</p>}
    {snapshot && <>
      <dl className="build-cache-metrics">
        <div><dt>当前缓存</dt><dd>{bytes(snapshot.totalBytes)}</dd></div>
        <div><dt>当前可清理</dt><dd>{bytes(snapshot.availableBytes)}</dd></div>
        <div><dt>累计已清理</dt><dd>{bytes(snapshot.cumulativeCleanedBytes)}</dd></div>
        <div><dt>累计扫描</dt><dd>{snapshot.scanCount} 次</dd></div>
      </dl>
      <p className="secondary">任务临时目录 <code>{snapshot.temporaryRoot}</code></p>
      <p className="secondary">提炼日志目录 <code>{snapshot.logDirectory}</code></p>
      {snapshot.pendingOperations > 0 && <p role="alert">有 {snapshot.pendingOperations} 条清理尚缺完成回执，请核对提炼日志和当前扫描结果。</p>}
      <div className="build-cache-table-wrap"><table>
        <thead><tr><th>缓存区域</th><th>体积</th><th>文件数</th><th>状态</th></tr></thead>
        <tbody>{snapshot.entries.map(entry => <tr key={entry.path}>
          <td>{entry.kind}<small>{entry.path}</small></td><td>{bytes(entry.bytes)}</td>
          <td>{entry.files}</td><td>{entry.active ? "使用中，清理时跳过" : "可清理"}</td>
        </tr>)}</tbody>
      </table></div>
      <details><summary>最近扫描与清理记录</summary><ul>{snapshot.recent.map(record =>
        <li key={record.id}>{new Date(record.at).toLocaleString()} · {record.kind} · 已清理 {bytes(record.bytesRemoved)}
          {record.errors.length > 0 && <span> · {record.errors.length} 项待处理</span>}
        </li>)}</ul></details>
      {snapshot.warnings.map((warning, index) => <p role="alert" key={index}>{warning}</p>)}
    </>}
    {message && <p role="status">{message}</p>}
    {error && <p role="alert"><ErrorText error={error} /></p>}
  </section>;
}
