import { useEffect, useRef } from "react";

export function resourceNames(value?: string[]): string[] {
  return (value ?? []).map(v => v.trim()).filter(Boolean);
}
export function resourceError(value?: string[], common?: string[]): string {
  const keys = resourceNames(value);
  if (keys.some(k => !/^[A-Za-z0-9_-]{1,80}$/.test(k))) return "资源名称只能包含英文字母、数字、下划线和短横线，长度为 1–80。";
  if (new Set(keys).size !== keys.length) return "同一列表内不能重复声明资源。";
  if (new Set([...resourceNames(common), ...keys]).size > 32) return "每项任务合并后的资源不能超过 32 项。";
  return "";
}
export function ResourceFields({id, value, common, onChange}: {
  id: string; value?: string[]; common?: string[]; onChange: (value: string[] | undefined) => void;
}) {
  const input = useRef<HTMLTextAreaElement>(null);
  const error = resourceError(value, common);
  useEffect(() => { input.current?.setCustomValidity(error); }, [error]);
  const effective = [...new Set([...resourceNames(common), ...resourceNames(value)])];
  return <section className="panel" aria-label={common ? "配置附加资源" : "项目共享资源"}>
    <label htmlFor={id}>{common ? "本配置附加资源（每行一个）" : "所有任务共享资源（每行一个，可选）"}</label>
    <textarea ref={input} id={id} rows={3} value={(value ?? []).join("\n")} aria-invalid={!!error} aria-describedby={`${id}-help`} onChange={e => onChange(e.target.value ? e.target.value.split(/\r?\n/) : undefined)}/>
    <p className="secondary" id={`${id}-help`}>资源名区分大小写，使用英文字母、数字、下划线或短横线，每个最多 80 字符，合并后最多 32 项。不同项目声明相同名称时会互斥。{common ? "附加资源与项目资源合并，同名项只计一次。" : "此处声明适用于全部任务，包括最终集成。"}名称与任务一起进入批准。经批准且资源不冲突的独立 worktree 任务可以并行；资源名只协调本工作台核心内的任务，不会自动锁定系统端口或数据库。</p>
    {error && <p role="alert">{error}</p>}
    {!!effective.length && <p>本项合并资源：{effective.join("、")}</p>}
  </section>;
}
