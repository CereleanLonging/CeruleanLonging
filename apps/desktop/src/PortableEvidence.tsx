import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

type Create = { action: "create"; root: string; index: string; indexSha256: string; destination: string };
type Verify = { action: "verify"; archive: string; manifestSha256: string };
type Request = Create | Verify;
type Result = {
  kind: "cerulean-portable-evidence-v1"; manifestSha256: string; sourceIndexSha256: string;
  filesVerified: number; bytesVerified: number; sourceWorkspaceRequired: false;
  credentialsContentRedacted: false; executionAllowed: false; restorableWorkbench: false;
  realityAttested: false; releaseAccepted: false;
};
type Api = (request: Request) => Promise<Result>;
const api: Api = request => invoke("portable_evidence", { request });
const digest = (value: string) => /^[a-f0-9]{64}$/.test(value);
const absolute = (value: string) => /^(?:[a-z]:[\\/]|\\\\)/i.test(value) && !/[\x00-\x1f]/.test(value);
const pathKey = (value: string) => value.replaceAll("/", "\\").replace(/\\+$/, "").toLowerCase();

export function PortableEvidence({ desktop, client = api }: { desktop: boolean; client?: Api }) {
  const [create, setCreate] = useState<Create>({ action: "create", root: "", index: "", indexSha256: "", destination: "" });
  const [verify, setVerify] = useState<Verify>({ action: "verify", archive: "", manifestSha256: "" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState<{ request: Request; value: Result } | null>(null);
  const locked = useRef(false), mounted = useRef(true), attempted = useRef(new Set<string>());
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  function editCreate(key: keyof Omit<Create, "action">, value: string) {
    setCreate(v => ({ ...v, [key]: value })); setResult(null); setError("");
  }
  function editVerify(key: keyof Omit<Verify, "action">, value: string) {
    setVerify(v => ({ ...v, [key]: value })); setResult(null); setError("");
  }
  async function run(request: Request) {
    if (!desktop || locked.current) return;
    if (request.action === "create") {
      const key = pathKey(request.destination);
      if (attempted.current.has(key)) return;
      attempted.current.add(key);
    }
    locked.current = true; setBusy(true); setResult(null); setError("");
    try {
      const value = await client(request);
      if (!mounted.current) return;
      if (value.kind !== "cerulean-portable-evidence-v1" || !digest(value.manifestSha256) || !digest(value.sourceIndexSha256)
        || !Number.isSafeInteger(value.filesVerified) || value.filesVerified <= 0 || value.filesVerified > 10000
        || !Number.isSafeInteger(value.bytesVerified) || value.bytesVerified < 0 || value.bytesVerified > 512 * 1024 * 1024
        || [value.sourceWorkspaceRequired, value.credentialsContentRedacted, value.executionAllowed,
          value.restorableWorkbench, value.realityAttested, value.releaseAccepted].some(v => v !== false)
        || (request.action === "create" ? value.sourceIndexSha256 !== request.indexSha256 : value.manifestSha256 !== request.manifestSha256)) {
        throw Error("返回内容与本次请求不一致，请保留原目录核查");
      }
      setResult({ request, value });
      if (request.action === "create") setVerify({ action: "verify", archive: request.destination, manifestSha256: value.manifestSha256 });
    } catch (e) {
      if (mounted.current) setError(`${request.action === "create" ? "创建未确认成功；目标可能已含部分或完整文件，请保留原目录，不覆盖或重发。" : "核验未通过；保留原文件，核对目录与独立保存的摘要。"}${String(e)}`);
    } finally {
      locked.current = false; if (mounted.current) setBusy(false);
    }
  }
  const repeated = attempted.current.has(pathKey(create.destination));
  return <section className="panel" aria-label="便携证据归档">
    <h2>便携证据归档</h2>
    <p>按已冻结的文件清单封存证据，并在其他目录核对内容是否完整。归档只保存在本机；请把清单摘要另行保存，以便之后核验。</p>
    <p>这不是工作台恢复备份。核验通过仅表示所选文件与摘要一致，不代表真实模型调用、费用或正式验收通过。文件可能包含原路径和用户内容，尚未做全文脱敏。</p>
    {!desktop && <p>请在桌面应用中使用。</p>}
    <fieldset disabled={!desktop || busy}>
      <legend>创建新归档</legend>
      <label>源证据根目录<input aria-label="源证据根目录" value={create.root} onChange={e => editCreate("root", e.target.value)} /></label>
      <label>冻结索引文件<input aria-label="冻结索引文件" value={create.index} onChange={e => editCreate("index", e.target.value)} /></label>
      <label>索引预期 SHA256<input aria-label="索引预期 SHA256" value={create.indexSha256} onChange={e => editCreate("indexSha256", e.target.value)} /></label>
      <label>新归档目录<input aria-label="新归档目录" value={create.destination} onChange={e => editCreate("destination", e.target.value)} /></label>
      <p>全部使用绝对路径；新目录的父目录须已存在，新目录须位于源证据根之外且尚不存在。不会覆盖已有目录。</p>
      {repeated && <p>本页已提交过此目录。请核对原归档；要创建另一份归档，请指定新目录。</p>}
      <button disabled={repeated || ![create.root, create.index, create.destination].every(absolute) || !digest(create.indexSha256)} onClick={() => void run({ ...create })}>创建证据归档</button>
    </fieldset>
    <fieldset disabled={!desktop || busy}>
      <legend>核验已有归档</legend>
      <label>待核验归档目录<input aria-label="待核验归档目录" value={verify.archive} onChange={e => editVerify("archive", e.target.value)} /></label>
      <label>清单预期 SHA256<input aria-label="清单预期 SHA256" value={verify.manifestSha256} onChange={e => editVerify("manifestSha256", e.target.value)} /></label>
      <p>使用创建时另行保存的清单摘要；不从待核验包自身取得预期值。核验不需要原工作区，也不执行归档文件。</p>
      <button disabled={!absolute(verify.archive) || !digest(verify.manifestSha256)} onClick={() => void run({ ...verify })}>核验所选归档</button>
    </fieldset>
    {busy && <p role="status">正在处理所选归档，请等待原结果。离开本页不取消已开始的操作；重新进入后先核对原目录。</p>}
    {error && <p role="alert">{error}</p>}
    {result && <div role="status" style={{ overflowWrap: "anywhere" }}>
      <p>{result.request.action === "create" ? "归档已创建并核验" : "所选归档核验通过"}：{result.request.action === "create" ? result.request.destination : result.request.archive}</p>
      <p>文件：{result.value.filesVerified}；字节：{result.value.bytesVerified}</p>
      <p>清单 SHA256（请另行保存）：<code>{result.value.manifestSha256}</code></p>
      <p>源索引 SHA256：<code>{result.value.sourceIndexSha256}</code></p>
    </div>}
  </section>;
}
