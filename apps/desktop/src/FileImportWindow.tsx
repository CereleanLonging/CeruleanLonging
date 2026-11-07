import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

type ImportStatus = { status: "waiting" | "cancelled" | "ready"; name?: string; bytes?: number };
const call = <T,>(request: object) => invoke<T>("file_import_window", { request });

export function FileImportWindow({ contextKey, disabled, onFile, onBusy }: {
  contextKey: string;
  disabled: boolean;
  onFile: (file: File) => void;
  onBusy: (busy: boolean) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const session = useRef<string | undefined>(undefined);
  const current = useRef({ contextKey, disabled });
  current.current = { contextKey, disabled };

  useEffect(() => {
    generation.current++;
    setBusy(false);
    onBusy(false);
    setError("");
    return () => {
      generation.current++;
      const previous = session.current;
      session.current = undefined;
      if (previous) void call({ operation: "cancel", session: previous }).catch(() => {
        // The native owner also closes its helper before workbench shutdown.
      });
    };
  }, [contextKey, disabled, onBusy]);

  async function cancel() {
    generation.current++;
    const previous = session.current;
    try {
      if (previous) await call({ operation: "cancel", session: previous });
      session.current = undefined;
      setBusy(false);
      onBusy(false);
    } catch (e) { setError(String(e)); }
  }

  async function start() {
    if (disabled || busy || document.querySelector("dialog[open]")) return;
    const attempt = ++generation.current;
    const context = contextKey;
    const active = () => attempt === generation.current && context === current.current.contextKey && !current.current.disabled;
    setError("");
    setBusy(true);
    onBusy(true);
    let id: string | undefined;
    let finished = false;
    try {
      const opened = await call<{ session: string }>({ operation: "start" });
      id = opened.session;
      if (!active()) return;
      session.current = id;
      let status: ImportStatus;
      do {
        await new Promise(resolve => window.setTimeout(resolve, 300));
        if (!active()) return;
        status = await call<ImportStatus>({ operation: "poll", session: id });
        if (!active()) return;
      } while (status.status === "waiting");
      if (status.status === "cancelled") return;
      if (!status.name || !Number.isSafeInteger(status.bytes) || status.bytes! < 0) throw Error("导入文件信息无效。");
      const parts: ArrayBuffer[] = [];
      let offset = 0;
      let done = false;
      while (!done) {
        const chunk = await call<{ bytes: number[]; offset: number; done: boolean }>({ operation: "read", session: id, offset });
        if (!active()) return;
        if (chunk.offset !== offset + chunk.bytes.length || chunk.offset > status.bytes!
          || (!chunk.done && chunk.bytes.length === 0)) throw Error("导入分块不完整。");
        parts.push(Uint8Array.from(chunk.bytes).buffer);
        offset = chunk.offset;
        done = chunk.done;
      }
      if (offset !== status.bytes) throw Error("导入文件长度不符。");
      await call({ operation: "finish", session: id });
      finished = true;
      if (active()) onFile(new File(parts, status.name, { type: /\.md$/i.test(status.name) ? "text/markdown" : "application/json" }));
    } catch (e) {
      if (active()) setError(String(e));
    } finally {
      let cleanupFailed = false;
      if (id && !finished) {
        try { await call({ operation: "cancel", session: id }); }
        catch (e) {
          cleanupFailed = true;
          if (active()) setError(`导入窗口收尾失败，请点击取消重试。${String(e)}`);
        }
      }
      if (active() && !cleanupFailed) {
        session.current = undefined;
        setBusy(false);
        onBusy(false);
      }
    }
  }

  return <div className="file-import-window">
    <button type="button" className="secondary" disabled={disabled || busy} onClick={() => void start()}>打开拖拽导入窗口</button>
    {busy && <button type="button" className="secondary" onClick={() => void cancel()}>取消导入</button>}
    {busy && <span role="status">请在普通权限窗口选择或拖入文件</span>}
    {error && <span role="alert">{error}</span>}
  </div>;
}
