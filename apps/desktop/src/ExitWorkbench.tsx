import { HelpHeading } from "./HelpHeading";
import { flushAccessStorage } from "./accessStorage";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
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
  const [unsavedAccess,setUnsavedAccess]=useState(false);
  const locked = useRef(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const returnButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open || !enabled || !dialog.current) return;
    const modal = dialog.current;
    const previousFocus = document.activeElement;
    // showModal promotes the dialog above every page stacking context and makes
    // the rest of the document inert, including keyboard/programmatic focus.
    modal.showModal();
    returnButton.current?.focus({preventScroll:true});
    return () => {
      modal.close();
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
    };
  }, [open, enabled]);
  useEffect(() => {
    if (!enabled) return;
    let disposed = false, unlisten: (() => void) | undefined;
    void api.listen(() => setOpen(true)).then(remove => { if (disposed) remove(); else unlisten = remove; })
      .catch(e => { if (!disposed) setError(`退出监听未就绪，请使用“退出工作台”按钮：${String(e)}`); });
    return () => { disposed = true; unlisten?.(); };
  }, [enabled, api]);
  async function act(exit: boolean, discardAccess=false) {
    if (locked.current) return;
    locked.current = true; setWaiting(true); setError("");
    try { if (exit) { if(!await flushAccessStorage()&&!discardAccess){setUnsavedAccess(true);throw Error("接入设置尚未保存，请返回工作台重试保存后再退出。");}await api.stop(); } else { await api.cancel(); setUnsavedAccess(false);setOpen(false); } }
    catch (e) { setError(String(e)); }
    finally { locked.current = false; setWaiting(false); }
  }
  if (!enabled) return null;
  return <>
    <button type="button" className="secondary-button workbench-exit-trigger" onClick={() => setOpen(true)}>退出工作台</button>
    {!open && error && <p role="alert"><ErrorText error={error}/></p>}
    {open && createPortal(<dialog ref={dialog} className="modal panel exit-workbench" aria-modal="true" aria-labelledby="workbench-exit-title" onCancel={e => { e.preventDefault(); void act(false); }}>
      <HelpHeading as="h2" topic="ExitWorkbench" id="workbench-exit-title" openOnFocus={false}>确认退出工作台？</HelpHeading>
      <p>确认后将先停止当前工作台的后台核心，核实退出后再关闭本窗口。未保存的编辑会丢失；已保存的项目、记录和凭证保留。</p>
      <p className="secondary">若有运行中或结果未确认的任务，退出会被阻止，请先在原任务处处理。独立维护窗口请先正常关闭。其他实验工作台和外部模型应用保持各自的运行状态。</p>
      {waiting && <p role="status">正在核对后台退出，请等待原结果…</p>}
      {error && <p role="alert"><ErrorText error={error}/></p>}
      {unsavedAccess&&<button type="button" className="secondary-button" disabled={waiting} onClick={()=>void act(true,true)}>放弃未保存的接入修改并退出</button>}
      <div className="integration-actions"><button ref={returnButton} type="button" autoFocus className="secondary-button" disabled={waiting} onClick={() => void act(false)}>返回工作台</button>
        <button type="button" className="primary" disabled={waiting} onClick={() => void act(true)}>确认退出并停止后台</button></div>
    </dialog>, document.body)}
  </>;
}
