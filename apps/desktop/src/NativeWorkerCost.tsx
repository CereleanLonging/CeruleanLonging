export function NativeWorkerCost({ adapter }: { adapter: string }) {
  if (adapter !== "codex" && adapter !== "omp") return null;
  return <p className="secondary" aria-label="原生执行费用">
    执行费用不可精确测量；按批准的请求次数与单次执行时限运行，金额以供应商或订阅账单为准。
  </p>;
}
