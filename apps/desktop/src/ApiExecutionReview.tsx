import type {ApiWorkerSelection} from "./contracts";
export function ApiExecutionReview({worker}:{worker:ApiWorkerSelection}) {
  const endpoint=worker.target.kind==="provider"?worker.target.endpoint:null;
  const destination=endpoint ? ({a6Api:"A6API 中转 · api.a6api.com",a6ApiVip:"A6API 中转 · api.a6api.vip",deepSeek:"DeepSeek 官方",anthropic:"Claude 官方",kimiChina:"Kimi 中国站",grok:"xAI Grok 官方",qwenBeijing:"Qwen 北京",qwenSingapore:"Qwen 新加坡",qwenHongKong:"Qwen 中国香港"}[endpoint.kind]) : "OpenAI 官方";
  return <dl>
    <dt>API 服务</dt><dd>{destination}{endpoint?.kind==="qwenBeijing"&&` · 业务空间 ${endpoint.workspaceId}`}</dd>
    <dt>执行模型</dt><dd>{worker.model}</dd>
    <dt>模型可用工具</dt><dd>{worker.fileTools.map(p=>p==="read-files"?"读取项目文件":"写入项目文件").join("、")}；验收命令由独立验收器运行</dd>
    {worker.fileTools.includes("write-files")&&<><dt>允许写入的文件</dt><dd>{worker.writeScope==="declared-artifacts"?"仅当前任务已批准的交付文件（准确路径）":"项目目录内文件"}</dd></>}
    <dt>单次任务上限</dt><dd>{worker.maxRounds} 轮模型请求 · {worker.maxToolCalls} 次文件调用</dd>
    <dt>单次请求上限</dt><dd>输出 {worker.maxOutputTokens.toLocaleString()} tokens · {worker.requestTimeoutSeconds} 秒</dd>
    <dt>用量停止阈值</dt><dd>{worker.maxTotalTokens.toLocaleString()} tokens；收到供应商用量后核对，达到阈值不再调用工具或发起下一轮。单次请求的输入用量可能使累计值超过阈值；当前不支持金额硬上限。</dd>
  </dl>;
}
