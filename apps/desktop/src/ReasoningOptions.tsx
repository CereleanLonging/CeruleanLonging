import { useEffect, useId, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ApiTarget, ReasoningEffort } from "./contracts";
import { apiTargetKey } from "./apiModels";

export const reasoningLabels: Record<ReasoningEffort, string> = {
  none: "关闭思考",
  minimal: "最低",
  low: "低",
  medium: "中",
  high: "高",
  xhigh: "很高",
  max: "最高",
  ultra: "超高",
};
export function reasoningLabel(effort?: ReasoningEffort | null) {
  return effort ? reasoningLabels[effort] : "默认";
}

export function ReasoningOptions({
  target,
  model,
  value,
  disabled,
  onChange,
  onReady,
  options: supplied,
  allowDefault = true,
}: {
  target: ApiTarget;
  model: string;
  value?: ReasoningEffort | null;
  disabled?: boolean;
  onChange: (effort: ReasoningEffort | undefined) => void;
  onReady?: (ready: boolean) => void;
  options?: ReasoningEffort[];
  allowDefault?: boolean;
}) {
  const name = useId();
  const [options, setOptions] = useState<ReasoningEffort[] | null>(
    supplied ?? null,
  );
  const [error, setError] = useState("");
  const identity = JSON.stringify([apiTargetKey(target), model, supplied]);
  useEffect(() => {
    let current = true;
    setError("");
    setOptions(supplied ?? null);
    if (!supplied)
      void invoke<ReasoningEffort[]>("model_reasoning_options", {
        target,
        model,
      })
        .then((result) => {
          if (current) setOptions(result);
        })
        .catch(() => {
          if (current) setError("思考选项读取失败，请返回后重新选择。");
        });
    return () => {
      current = false;
    };
  }, [identity]);
  useEffect(() => {
    onReady?.(!!options && (value ? options.includes(value) : allowDefault));
  }, [options, value, allowDefault, onReady]);
  return (
    <fieldset className="reasoning-options" disabled={disabled || !options}>
      <legend>思考强度</legend>
      {options ? (
        <>
          {allowDefault && (
            <label>
              <input
                type="radio"
                name={name}
                checked={!value}
                onChange={() => onChange(undefined)}
              />
              默认
            </label>
          )}
          {options.map((effort) => (
            <label key={effort}>
              <input
                type="radio"
                name={name}
                checked={value === effort}
                onChange={() => onChange(effort)}
              />
              {reasoningLabels[effort]}
            </label>
          ))}
          {!options.length && (
            <p className="secondary">
              {allowDefault
                ? "当前接入尚未提供已确认的可调档位，使用模型默认配置。"
                : "该模型的思考档位尚未确认，请返回核对模型，或配置对应服务的 Key。"}
            </p>
          )}
          {value && !options.includes(value) && (
            <p role="alert">原思考档位与此模型不兼容，请重新选择。</p>
          )}
        </>
      ) : (
        <p role={error ? "alert" : "status"}>
          {error || "正在读取模型支持的思考选项…"}
        </p>
      )}
    </fieldset>
  );
}
