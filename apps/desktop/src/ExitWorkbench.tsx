import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ErrorText } from "./ErrorText";

export type ExitApi = { listen: (handler: () => void) => Promise<() => void>; stop: () => Promise<void>; cancel: () => Promise<void> };
const nativeApi: ExitApi = {
  listen: handler => getCurrentWindow().listen("workbench-close-requested", handler),
  stop: () => invoke("close_workbench"),
  cancel: () => invoke("cancel_workbench_exit"),
};
export function ExitWorkbench({ enabled, api = nativeApi }: { enabled: boolean; api?: ExitApi }) {
  const [open, setOpen] = useState(false), [waiting, setWaiting] = useState(false), [error, setError] = useState("");
  const locked = useRef(false);
  useEffect(() => {
    if (!enabled) return;
    let disposed = false, unlisten: (() => void) | undefined;
    void api.listen(() => setOpen(true)).then(remove => { if (disposed) remove(); else unlisten = remove; })
      .catch(e => { if (!disposed) setError(`退出监听未就绪，请使用“退出工作台”按钮：${String(e)}`); });
    return () => { disposed = true; unlisten?.(); };
  }, [enabled, api]);
  async function act(exit: boolean) {
    if (locked.current) return;
    locked.current = true; setWaiting(true); setError("");
    try { if (exit) await api.stop(); else { await api.cancel(); setOpen(false); } }
    catch (e) { setError(String(e)); }
    finally { locked.current = false; setWaiting(false); }
  }
  if (!enabled) return null;
  return <>
    <button type="button" className="secondary-button" onClick={() => setOpen(true)}>退出工作台</button>
    {!open && error && <p role="alert"><ErrorText error={error}/></p>}
    {open && <div className="modal-backdrop"><section className="modal panel exit-workbench" role="dialog" aria-modal="true" aria-labelledby="workbench-exit-title">
      <h2 id="workbench-exit-title">确认退出工作台？</h2>
      <p>确认后将先停止当前工作台的后台核心，核实退出后再关闭本窗口。未保存的编辑会丢失；已保存的项目、记录和凭证保留。</p>
      <p className="secondary">若有运行中或结果未确认的任务，退出会被阻止，请先在原任务处处理。独立维护窗口请先正常关闭。其他实验工作台和外部模型应用保持各自的运行状态。</p>
      {waiting && <p role="status">正在核对后台退出，请等待原结果…</p>}
      {error && <p role="alert"><ErrorText error={error}/></p>}
      <div className="integration-actions"><button type="button" autoFocus className="secondary-button" disabled={waiting} onClick={() => void act(false)}>返回工作台</button>
        <button type="button" className="primary" disabled={waiting} onClick={() => void act(true)}>确认退出并停止后台</button></div>
    </section></div>}
  </>;
}
