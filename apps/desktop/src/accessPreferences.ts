import { useEffect, useState } from "react";
import { setAccessItem } from "./accessStorage";

// UI choices only. Credentials and connection results stay in the core vault/status.
export function preferenceKey(scope: string | undefined, name: string) {
  return scope ? JSON.stringify(["cerulean-access-preference", 1, scope, name]) : null;
}
export function readPreference(key: string | null): string | undefined {
  if (!key) return;
  try { const value = JSON.parse(localStorage.getItem(key) ?? "null"); return typeof value === "string" && value.length <= 8192 ? value : undefined; }
  catch { return; }
}
export function writePreference(key: string | null, value: string): boolean {
  if (!key) return true;
  return setAccessItem(key, JSON.stringify(value));
}
export function useAccessPreference(scope: string | undefined, name: string, fallback: string, allowed?: readonly string[]) {
  const key = preferenceKey(scope, name);
  const read = () => { const value = readPreference(key); return value !== undefined && (!allowed || allowed.includes(value)) ? value : fallback; };
  const [state, setState] = useState(() => ({key, value:read(), error:false}));
  useEffect(() => { setState({key, value:read(), error:false}); }, [key]);
  const value = state.key === key ? state.value : read();
  const setValue = (next: string) => {
    if (allowed && !allowed.includes(next)) return;
    setState({key, value:next, error:!writePreference(key, next)});
  };
  return [value, setValue, state.key === key && state.error] as const;
}
