import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { DesktopObservation } from "./contracts";
import { ErrorText } from "./ErrorText";

type Request =
  | { kind: "codexSession"; sessionId: string; executable?: string | null }
  | { kind: "omp"; executable: string; model: string; runtimeParent?: string | null }
  | { kind: "qoder"; observation: DesktopObservation };
type Result = { message: string; checkedAt: string };
type Props = { request: Request | null; busy: boolean; scope: string; hint?: string };

export function LocalConnectionTest(props: Props) {
  // A result belongs to exactly one configuration, including its workbench.
  return <ConnectionTest key={JSON.stringify([props.scope, props.request])} {...props}/>;
}

function ConnectionTest({ request, busy, hint }: Props) {
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<Result | null>(null);
  const [error, setError] = useState("");
  const lock = useRef(false);
  async function test() {
    if (busy || lock.current || !request) return;
    lock.current = true;
    setTesting(true);
    setResult(null);
    setError("");
    try {
      setResult(await invoke<Result>("test_local_connection", { request }));
    } catch (error) {
      setError(String(error));
    } finally {
      lock.current = false;
      setTesting(false);
    }
  }
  return <div className="connection-test" data-connection-actions>
    <button type="button" className="primary" disabled={busy || testing || !request} onClick={() => void test()}>
      {testing ? "测试中…" : "测试连通"}
    </button>
    <div className="connection-test-feedback">
      {hint && <p className="secondary">{hint}</p>}
      {testing && <p role="status">正在验证当前接入，请等待测试结果…</p>}
      {result && <p role="status">测试通过 · {result.message} {new Date(result.checkedAt).toLocaleString("zh-CN")}</p>}
      {error && <p role="alert">测试未通过 · <ErrorText error={error}/></p>}
    </div>
  </div>;
}
