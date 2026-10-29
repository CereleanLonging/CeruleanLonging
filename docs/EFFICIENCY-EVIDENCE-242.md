# 242 原账本、API请求与独立验收的只读导出

2026-10-21 02:19 +08:00。原始证据读取器和独立导出工具已完成，本轮0模型调用、0任务派发、0新增验收执行。真实效率实验仍0/72，`releaseAccepted=false`。

## 变更与复用

新增`crates/core/src/efficiency_evidence.rs`及`crates/core/examples/export_efficiency_evidence.rs`。复用DurableWorkflow的锁定原账本重放、CallInventory的原批准/分配/逐轮链检查，以及verification_detail的原Verify事件、验收规范、阶段文件和日志摘要核查。没有另写一套JSON哈希重放器。

CallInventory新增专门的只读inspect入口及只读journals访问器。原DurableWorkflow::inspect_existing保持写保护状态；普通运行路径的requires_reopen检查不变，没有为导出清除poisoned标记。导出器不读取凭证、不恢复投影、不结算费用、不分派或运行验收，也没有新增IPC/UI操作。

调用需要原数据目录、项目ID、预期方案摘要和预期账本末端摘要。锁被其他写者持有、身份/摘要不匹配、账本或请求链损坏时返回错误。示例工具：

```text
target/debug/examples/export_efficiency_evidence.exe <data-root> <project-id> <revision-digest> <ledger-head>
```

输出为JSON，不自动写入实验记录或批准文件。实际二进制和摘要见`reports/efficiency-evidence-242/example-executable.json`。本轮对240原serial数据调用了这个可执行程序，返回2条原请求/响应记录，未改变数据。

## 证据范围

导出保留原任务状态、API意图/响应数量、逐轮身份/日志末端、供应商原始用量字段、请求模型与实际模型、已记录独立验收的关联与日志摘要。API意图数量不直接等于远端计费请求数量。响应ID只输出摘要，不输出模型回答、工具参数或日志正文。

未知/失败用量保持未知，实际费用和实验墙钟为null。供应商特有的缓存字段保留原始含义，后续聚合不能将缺失当零，也不能跨供应商直接相加解释。原账本时间范围不等于含用户交互和规划的完整实验墙钟。

`ledgerAndApiChainsVerified=true`和`recordedVerificationFilesVerified=true`仅证明本地链及已经记入账本的验收文件关联。它们不证明：

- 日志来自真实模型；合成响应同样可以形成合法日志，所以`realityAttested=false`。
- 管理用量或供应商账单；本轮`managementUsageIncluded=false`，不处理需要金额账本的运行。
- 所有待处理验收均已结束；未记入Verify的作业不能据此宣称完成。
- 所有当前产物文件已重新逐字节核验，或文件工具日志已完整重放；本轮复用原完成封存关联及验收证据读取，不重复执行产物验收。
- 已绑定239任务集/seed、241准备目录、真实模型配置、用户授权和完整计时；`experimentContentFullyVerified=false`。

因此不要直接把该输出当作239统计器的完整真实运行记录，也不要据此宣布提效。

## 实际验证

一个定向Rust测试覆盖13种情形：

| 类型 | 数量 | 结果 |
| --- | ---: | --- |
| 回读240四份原数据 | 4 | 原18条API响应记录、8次验收记录全部可关联；错误集成的1次拒绝保持 |
| 方案摘要、账本末端、项目路径和写锁冲突 | 4 | 全部拒绝 |
| API链篡改、验收日志篡改、孤立API目录、账本事件缺失 | 4 | 在隔离副本中全部拒绝 |
| 合成中断前缀，仅有意图无响应 | 1 | 保留Dispatching、未知用量与null费用，不冒充通过 |

四份原数据在检查和独立工具执行前后逐文件摘要一致。没有重跑240任务或独立验收。构建、最终格式、core全target严格Clippy通过；未重跑全Rust套件。测试二进制对应源码与最终源码均已归档，测试后仅调整lib.rs模块顺序以满足rustfmt，最终源码通过Clippy与独立工具构建/回读。

保留开发过程证据：第一次复制隔离副本遇到WinError206，原部分副本留在corruptions/api-chain，后用扩展路径创建corrected-corruptions；第一份格式检查因模块顺序失败，原回执保留。原240恢复测试二进制已在本轮归档，避免target构建覆盖旧可追溯产物。

全部本轮调用进程已退出、日志句柄关闭已核实。0.1.15包清单摘要保持；本次增加的是只读库工具与独立开发工具，未重打包或安装。

证据：[结果与索引](../reports/efficiency-evidence-242/result.json)、[13种情形结果](../reports/efficiency-evidence-242/test-result.json)、[完整需求状态](../reports/efficiency-evidence-242/requirements.json)。finalize.py及已执行的prepare/build/run-tests/checks不要原样重跑；现有输入、日志和输出都保留。

## 仍需推进

单执行者基线尚未实现。本轮进一步核实：底层工作流的Propose/Assign/Accept有固定角色约束，执行配置还要求唯一的主/副指挥绑定；不能将本地脚本伪装为OpenAI管理模型来消除开销。后续须为基线明确区分本地确定性控制与真实管理决策，复用文件工具、请求日志和验收机制，保留现有产品的角色、批准和防重派约束。

可以独立继续管理用量/原操作链的只读绑定，以及任务/seed/配置/授权和完整计时的实验记录关联；复用本轮读者，不重新解析原始链或再次验收旧任务。全部分支验收接线、真实角色模型、单槽/总预算和失败停跑规则也仍需完成，之后才提交真实实验方案。

OMP196、WorkBuddy206、三层218、Qoder232/238及237包成果保持。其余完整供应商/故障矩阵、严格桌面监督、金额门禁、安装/独立Windows/正式签名门槛未被本轮替代。当前账户实时额度不可可靠读取，95%收尾规则继续有效。
