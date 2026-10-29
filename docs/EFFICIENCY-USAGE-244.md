# 244 用量口径、跨通道去重与未知保留

2026-10-21 02:40 +08:00。新增`scripts/efficiency-usage.py`，用于处理242/243原始导出。它不调用模型、不调度、不估算费用，也不产生已验收的实验记录。真实实验仍0/72。

## 行为

工具要求输入文件与指定SHA256一致，输出文件必须不存在。它检查项目/请求身份、同供应商响应摘要是否在执行者或管理通道重复、请求与用量状态是否一致，并保留未完成、失败、丢弃和外部会话范围。请求模型与实际模型不同会单独列出，不默默视为同配置。

用量按原始来源解释：

| 来源 | 输入token口径 |
| --- | --- |
| OpenAI及DeepSeek等兼容API观察 | inputTokens已含缓存；不再加cacheReadTokens |
| Anthropic观察 | inputTokens不含缓存读/写；两类字段齐全时才相加，缺失则完整输入未知 |
| OpenAI scoped标准化用量 | input是非缓存输入，加cacheRead与cacheWrite得到完整输入 |
| 外部Codex会话 | 本读取链没有会话计量，保持未知，禁止填造用量 |

每项输出同时保留已知小计、缺失范围数和完整总数。存在未知时完整总数为null。缺少整个管理导出或存在复核残片时，总表和供应商分组都保留不完整状态；分组明确只覆盖已导出的范围。原始usage也保留，以免丢失Anthropic已知非缓存输入等局部信息。

`recordedScopes`不是模型调用数。例如218包含4个执行者请求范围和3个管理操作范围，共7个范围，其中2个是外部Codex会话操作，不能说成7次API调用。

CLI：

```text
python -B -X utf8 scripts/efficiency-usage.py --input <243导出JSON> --expected-sha256 <已核对摘要> --output <不存在的新文件>
```

## 已完成检查

首轮18项检查通过，覆盖缓存重复计入、Anthropic字段缺失、scoped口径、非法/溢出计数、跨通道与通道内重复响应、重复操作、项目错配、模型差异、丢弃/失败/未知保留，以及摘要选择与防覆盖。首轮还完成2次实际CLI处理。

复核后补充供应商分组的不完整标识：之前全局总数正确保留未知，但分组容易被误读为完整。保留首轮源码和输出，增加2项定向检查（其中1项扩展原测试、1项新增），并用最终工具再处理两个原输入。累计20次检查执行、19个不同测试名；不是20次模型实验。

218原五条已知DeepSeek响应仍得到输入9406、输出1039、缓存命中2816小计；因2个外部会话用量未知，整条链的完整总数保持null。合成丢弃例的17个输入token仍计入，未知操作不删除、不填零。

当前权威入口为[latest.json](../reports/efficiency-usage-244/latest.json)，指向`*.refined.usage.json`，不是首轮输出。另见[结果](../reports/efficiency-usage-244/result.json)、[完整需求状态](../reports/efficiency-usage-244/requirements.json)。18项首轮和2项补充记录分别在test-result.json、refinement-test-result.json。

所有6个调用进程已退出且日志关闭，输入导出摘要不变；0新模型/派发/验收，不重读原应用会话，不重跑旧任务。未改Rust、未增加target构建数据、未改变0.1.15包。run/refine/finalize均已完成，不能原样重跑覆盖旧结果。

## 尚未完成

此工具只核对所选导出的字节与统计结构，不重新认证其原账本，也不证明导出目录包含历史上所有操作。`upstreamExportContentReauthenticated=false`、`realExecutionAttested=false`、`wholeRunUsageComplete=false`和`experimentRecordReady=false`均保持。不能把输出直接当239统计器要求的完整真实实验记录。

任务规范、初始seed、真实模型/角色配置、授权范围、启动前管理清单与全过程计时，仍需与242/243原始证据绑定到不可覆盖的实验记录。不能将218/240的旧回读装入239的72个槽。真实单执行者基线与完整分支验收接线仍缺，之后还需冻结模型和单槽/总预算，取得真实实验授权。

原接入/编程/候选成果与完整签名、安装、独立Windows、供应商/故障矩阵等门槛保留；releaseAccepted=false、目标active。账户实时额度仍无法可靠读取，token统计不作为账户额度替代。
