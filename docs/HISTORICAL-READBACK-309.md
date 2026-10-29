# 309 原真实历史的严格只读回查

承接 307 的历史回查计划及 308 的查询读取边界。四个原数据根含 canonical 路径及 Windows 用户绑定的读者 guard，直接复制到新路径不能代表有效回查；本轮保留原目录、原 guard 和原任务，使用现有严格只读入口。

## 原证据与执行范围

| 原阶段 | 项目 | 工作流事件 | 需要保持的结果 |
|---|---|---:|---|
| 218 | chief218 | 8 | sum accepted / verified，3 条管理决策 applied |
| 284 | relay284 | 0 | 工作流为空，1 条管理决策 unknown |
| 286 | relay286 | 8 | sum accepted / verified，3 条管理决策 applied |
| 295 | relay295 | 8 | product accepted / verified，3 条管理决策 applied |

期望取自冻结的 297 回查记录；准备时先比对原目录每个文件与 297 保存的摘要，以及三个原编程产物摘要。284 的 Unknown 来自管理记录，不能用空工作流证明。

专用测试默认 ignored，仅在显式计划和新报告路径下运行。它持有已存在的根 writer.lock，调用当前 `data_format::status` 核对原路径、SID、schema 和读者标记；管理记录通过 `DecisionJournal::read_existing` 全读，工作流通过现有 `DurableWorkflow::inspect_existing` 持有其自身 writer.lock、执行 `load(false)` 全重放。工作流仍检查当前上下文和金额/权限 guard。

核对工作流 state/revision/approval/sequence，与每条管理决策的 operationId/projectId/purpose/state/evidenceDigest。只读工作流不使用检查点；管理记录若有待发布残片则停止另行核查，不在此处恢复。没有调用 `Store::open` 或 `RuntimeHost::open`，没有模型、派发、验证命令、Accept、目录创建或投影发布。

外层驱动在执行前后核对完整文件集合、目录集合和文件 SHA256；即使测试失败也保存后置清单。原数据不含 `.dpapi`，准备程序遇到凭证文件或链接/reparse 点会停止。结果只写到新 `reports/historical-readback-309`。

## 证据与限制

计划与原始摘要见 `reports/historical-readback-309/plan.json` 及各阶段 `*-prepared.json`。实际通过与否以 `result.json`、`observed.json`、`integrity-result.json` 和原日志为准。首次准备仅因本机 Python 不支持 `Path.is_junction` 在读取原文件前退出，改为 Windows reparse 属性核查；未将该问题归因于原数据，失败记录保留。

本轮证明当前核心的严格只读重放兼容，不能代替完整 Host 冷启动、SQLite/DPAPI 快速路径、新包安装/升级或新的真实任务验收；不增加真实效率样本。297 原候选的完整 Host 回查证据保留，各阶段范围分别记录。正式验收仍未完成。
