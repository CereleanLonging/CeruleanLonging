import { HelpHeading } from "./HelpHeading";
import { ErrorText } from "./ErrorText";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Entry = { id: string; directory: string; ready: boolean; error: string | null };
type Catalog = { currentLabel: string; currentDirectory: string; catalogRoot: string; entries: Entry[] };
type Api = {
  read: () => Promise<Catalog>;
  create: (id: string) => Promise<Entry>;
  open: (id: string) => Promise<unknown>;
};
const api: Api = {
  read: () => invoke("experiment_workbenches"),
  create: id => invoke("create_experiment_workbench", { id }),
  open: id => invoke("open_experiment_workbench", { id }),
};
export function ExperimentWorkbenches({ desktop, client = api, inline = false }: { desktop: boolean; client?: Api; inline?: boolean }) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [visible, setVisible] = useState(false);
  const [id, setId] = useState("exp-");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const locked = useRef(false);
  const attempted = useRef(new Set<string>());
  useEffect(() => {
    let stopped = false;
    if (desktop) void client.read().then(v => { if (!stopped) setCatalog(v); })
      .catch(e => { if (!stopped) setError(String(e)); });
    return () => { stopped = true; };
  }, [desktop, client]);
  async function action(run: () => Promise<void>) {
    if (locked.current || !desktop) return;
    locked.current = true; setBusy(true); setError(""); setNotice("");
    try { await run(); } catch (e) { setError(String(e)); }
    finally { locked.current = false; setBusy(false); }
  }
  const valid = /^exp-[a-z0-9-]{1,44}$/.test(id) && !id.endsWith("-");
  const current = catalog?.currentLabel === "main" ? "日常工作台" : catalog?.currentLabel.replace(/^experiment-/, "") ?? "读取工作台…";
  return <div>
    <p style={{ overflowWrap: "anywhere" }}>{desktop ? current : "浏览器预览"}</p>
    {!inline && <button className="secondary-button" disabled={!desktop} onClick={() => setVisible(true)}>实验工作台</button>}
    {(inline || visible) && <div className={inline ? undefined : "modal-backdrop"}><section className={inline ? "panel" : "modal panel experiment-workbench-dialog"} role={inline ? "region" : "dialog"} aria-modal={inline ? undefined : true} aria-label={inline ? "独立实验工作台管理" : "独立实验工作台"} style={inline ? undefined : { maxHeight: "85vh", overflow: "auto" }}>
      <HelpHeading as="h2" topic="ExperimentWorkbenches">实验工作台管理</HelpHeading>
      <p>实验在独立窗口和数据目录中运行。打开其他工作台会保留当前窗口及后台任务；退出时按所属窗口的流程核对并停止后台。</p>
      <p style={{ overflowWrap: "anywhere" }}>当前：{catalog?.currentDirectory ?? "未读取"}</p>
      <p>新建时会保存空目录冷备份并启用实验数据保护。凭证需在新窗口的接入管理中单独配置；创建和打开均不派发模型任务。</p>
      <p style={{ overflowWrap: "anywhere" }}>保存位置：{catalog?.catalogRoot ?? "未读取"}。固定实验材料的 dataRoot 应与所选工作台数据目录一致。</p>
      {error && <p role="alert"><ErrorText error={error}/></p>}
      {notice && <p role="status">{notice}</p>}
      <label>实验工作台编号<input aria-label="实验工作台编号" value={id} disabled={busy} maxLength={48} onChange={e => setId(e.target.value)} placeholder="exp-example" /></label>
      <p>以 exp- 开头，仅用小写字母、数字和连字符，5 至 48 位。已有或未完成的编号不能重复创建。</p>
      <div className="experiment-workbench-actions">
      <button className="primary" disabled={busy || !catalog || !valid || attempted.current.has(id) || catalog.entries.some(e => e.id === id)} onClick={() => void action(async () => {
        attempted.current.add(id);
        let entry: Entry;
        try { entry = await client.create(id); }
        catch (e) {
          const originalError = `创建请求未确认成功，不能据此判断目录未创建。请核对原登记；不要重新提交 ${id}：${String(e)}`;
          setError(originalError);
          try { setCatalog(await client.read()); }
          catch (readError) { setError(`${originalError}。原登记回读也未成功，请稍后点击“读取原登记”：${String(readError)}`); }
          return;
        }
        setNotice(`已创建 ${entry.id} 并启用保护。请选择打开；尚未开始实验。`);
        try { setCatalog(await client.read()); }
        catch (e) { setError(`创建已返回成功，列表刷新失败；请读取原登记，不要重新创建：${String(e)}`); }
      })}>创建并启用实验保护</button>
      <button className="secondary-button" disabled={busy} onClick={() => void action(async () => setCatalog(await client.read()))}>读取原登记</button>
      <button className="secondary-button" disabled={busy} onClick={() => void action(async () => { await client.open("main"); setNotice("已打开日常工作台窗口。"); })}>打开日常工作台</button>
      </div>
      {catalog?.entries.map(entry => <article key={entry.id} className="panel">
        <HelpHeading as="h3" topic="ExperimentWorkbenches">{entry.id} · {entry.ready ? "可打开" : "需核查"}</HelpHeading>
        <p style={{ overflowWrap: "anywhere" }}>{entry.directory}</p>
        {entry.error && <p role="alert">{entry.error}。原目录保留，不自动修复或重建。</p>}
        <button className="secondary-button" disabled={busy || !entry.ready} onClick={() => void action(async () => { await client.open(entry.id); setNotice(`已打开 ${entry.id} 窗口，当前工作台保持。`); })}>打开 {entry.id}</button>
        {!entry.ready && <>
          <p>保留原目录用于核查。若要继续其他实验，请使用新编号单独创建；原目录不会被删除、修复或重新执行。</p>
          <button className="secondary-button" disabled={busy} onClick={() => { setId("exp-"); setNotice(`已保留 ${entry.id} 的原现场。请在上方填写新编号，再决定是否创建；本按钮尚未创建任何工作台。`); }}>保留 {entry.id}，填写新编号</button>
        </>}
      </article>)}
      {!inline && <button className="secondary-button" disabled={busy} onClick={() => setVisible(false)}>关闭选择面板</button>}
    </section></div>}
  </div>;
}
