import type { DesktopBinding, DesktopTrial, DesktopWindow } from "./contracts";

export function qoderCandidate(version: string): boolean {
  const parts = version.split(".");
  return parts.length === 4 && parts[0] === "0" && parts[1] === "4"
    && parts.slice(2).every(part => /^\d+$/.test(part) && Number(part) <= 65535)
    && Number(parts[2]) >= 3;
}

export function qoderEstablished(version: string): boolean {
  return version === "0.4.3.0" || version === "0.4.8.0";
}

function sameWindow(left: DesktopWindow, right: DesktopWindow): boolean {
  return left.app === right.app && left.hwnd === right.hwnd && left.pid === right.pid
    && left.startTicks === right.startTicks && left.exe === right.exe
    && left.title === right.title && left.version === right.version;
}

export function qoderUpgradeBlock(binding: DesktopBinding, trials: DesktopTrial[], bridge: string): string | null {
  const current = binding.observation;
  if (current.window.app !== "qoder" || !qoderCandidate(current.window.version)) {
    return "当前桌面版本超出 Qoder 0.4 接口适配范围，请等待对应适配器更新。";
  }
  if (qoderEstablished(current.window.version)) return null;
  const passed = trials.some(trial => {
    const proof = trial.proof;
    const previous = trial.review.observation;
    return !!proof && binding.bridgeSha256 === bridge && trial.review.bridgeSha256 === bridge
      && proof.readerSha256 === bridge && sameWindow(previous.window, current.window)
      && previous.sessionId === current.sessionId && previous.modelLabel === current.modelLabel
      && proof.marker === trial.review.marker && proof.sessionId === current.sessionId
      && JSON.stringify(proof.baseline) === JSON.stringify(trial.review.baseline)
      && proof.promptSeen && proof.completed && proof.replyMatches
      && Date.parse(proof.observedAt) <= Date.parse(current.capturedAt);
  });
  return passed ? null : "升级版本待验证。请在高级会话工具中完成固定消息验证，再重新读取并保存关联、批准方案。";
}
