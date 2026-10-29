# 243 管理来源、原用量与工作流关联导出

2026-10-21 02:32 +08:00。新增管理记录读取已通过，继承242执行者/验收证据导出。0新增模型调用、任务派发或验收执行；真实效率实验仍0/72，正式验收未完成。

## 实现与范围

新增`management_execution/efficiency.rs`，复用DecisionJournal原记录解析、来源校验与应用记录。新增`efficiency_evidence::read_with_management`，读取现有数据目录writer.lock并独占锁定，然后在原工作流锁下完成执行者、验收和管理记录检查。它不创建锁、不启动核心、不读Key、不恢复记录或结算费用；原四参数读取方式保持可用。

独立工具新增可选参数：

```text
target/debug/examples/export_efficiency_evidence.exe <data-root> <project-id> <revision-digest> <ledger-head> --include-management
```

工具与摘要见`reports/efficiency-management-243/example-executable.json`。这仍是离线开发工具，没有新增UI/IPC写入口或新调度器。

导出保留OpenAI、DeepSeek和外部Codex会话三种来源，包含原操作ID、用途、状态、请求/实际模型、原用量形状、意图/响应/链摘要，以及实际工作流事件关联。已丢弃、未应用和未知记录同样保留；不会因为交付失败删除其开销。

对于管理应用，按原操作ID查找工作流事件并核对身份、命令、完整上下文和已应用回执哈希；反向检查工作流中每个mgmt事件都有原管理来源。不同管理操作复用同供应商响应身份时拒绝，防止把重复响应当成独立开销。输出响应ID摘要，不输出管理指令全文、决策理由或原日志正文。

## 实际验证

定向Rust测试分8组：读取240原合成记录与218原真实闭环记录；拒绝管理链篡改、合法链指向错误工作流事件、整个管理操作缺失、重复响应身份四类副本；保留已丢弃用量和未知请求；拒绝活动根写者。首次运行因新增合成夹具的响应ID没有`resp_`前缀而失败，已保留原日志和副本，修正新副本后同一小型只读测试通过，产品规则未放宽。

格式、core全target严格Clippy、独立工具构建通过。实际工具分别执行了带管理选项的218回读和四参数兼容回读；两个原数据目录前后逐文件摘要相同。未重跑原任务或验收器，未重跑全Rust套件。构建/检查进程已退出且日志关闭，原242测试和工具二进制已归档保留，0.1.15包未改动。

218回读得到原4次执行者响应和1次DeepSeek副指挥响应，其五条记录的输入9,406、输出1,039、缓存命中2,816 tokens与原218交付结果一致。缓存命中已包含在输入中，不再次加总。另有2个Codex会话操作，其用量未知，不能据此声称整条链的总token或总费用完整。

证据：[结果与索引](../reports/efficiency-management-243/result.json)、[修正后测试](../reports/efficiency-management-243/corrected-read/test-result.json)、[218用量对照](../reports/efficiency-management-243/real218-usage-reconciliation.json)、[完整需求状态](../reports/efficiency-management-243/requirements.json)。

## 边界与续接

`managementUsageIncluded=true`只表示纳入原记录中可读的观察，`usageComplete=false`仍保持。OpenAI旧观察没有完整wire/缓存字段时保留来源标签；DeepSeek/API与scoped标准化usage结构不同，不可盲目相加。Codex会话用量和套餐成本保持null，实际费用、管理请求耗时和全过程墙钟也没有推算。

本地有效记录不能自行证明真实调用，所以`realityAttested=false`。218的真实来源结论继承其原授权/执行证据，本轮仅重读，没有增加一次真实效率实验。完整实验还需要绑定授权、配置、任务/seed、起止时间以及已冻结的管理清单；例如删除整个未应用且没有工作流事件的操作，不能只靠当前目录证明其从未存在。管理与执行者跨通道的响应去重也应在实验级绑定中检查，本轮对218五条已知响应单独确认摘要互异。

下一步将239/241准备材料、242/243来源摘要接入不可覆盖的实验记录，保留失败、取消、未知与未运行槽；不能提前把metrics的完整证据认证标志改为true。真实单执行者基线、全部分支验收接线、实际模型/预算与真实实验授权继续推进。底层主/副指挥角色约束仍保持，不能伪造OpenAI管理身份来生成无开销基线。

本轮所有脚本已结束，不原样重跑prepare/build/run-tests/fix-fixture/checks/finalize；失败和修正证据并存。原OMP、WorkBuddy、三层218、Qoder232/238和237包结论保持。完整供应商/故障矩阵、严格桌面监督、金额门禁、安装/独立Windows/正式签名等门槛未被替代。账户实时额度不可可靠读取，95%规则保持。
