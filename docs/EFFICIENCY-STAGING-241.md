# 241 真实实验前的初始工作区准备入口

2026-10-21 02:06 +08:00。72个槽的准备检查通过；真实实验0/72，`releaseAccepted=false`。继承239任务集和240选定Host演练，不重新执行模型、桌面或编程验收。

## 实现

新增`scripts/prepare-efficiency-workspace.py`。它核对239修正后的任务/槽清单、验收器和240启动器固定摘要，只读取选定任务的seed、task.txt和verify.json。未使用含参考实现的240 prepared.json。

每个新目录包含：

- `project/`：仅声明的初始mjs文件，与槽seed摘要一致。
- `control/task.txt`、`control/verify.json`：原始任务说明和独立检查规范，置于执行项目外。
- `control/host-inputs.json`：待填的Host配置材料，单执行者覆盖全部文件，团队保留原分解。它不是ProjectSetupDraft或已批准计划，不可直接提交为执行命令。
- `ready.json`：最后写入的准备完成记录，含全部输出摘要；`readyToDispatch=false`、`approved=false`。此处ready只表示文件准备完成。

工具不创建模型客户端、不初始化Git、不批准、不分派，也不调度。执行前仍需固定Node和模型配置、配置Host/worktree、登记独立验收器、取得真实运行范围授权并批准当前方案。所有实验组必须使用相同执行者配置与完整任务规范。

示例（仅在确需新的准备目录时使用，不重复已生成的槽）：

```powershell
python -B -X utf8 scripts/prepare-efficiency-workspace.py --slot P04-r1-single --destination D:\test\CeruleanLonging\reports\efficiency-staging-241\prepared\new-review-P04-single
```

目标父目录须存在，目标自身必须不存在。工具拒绝链接/重解析点、越界路径、冻结输入摘要变化和已有目录；中途失败保留原现场，不生成ready，不自动清理或重试。没有复制参考答案不等于OS访问隔离，实际适配器仍须通过原文件权限机制阻止项目外读取。

## 已完成检查

一次检查进程，7项检查通过：

1. 为72个计划槽生成独立准备目录，逐个核对seed/规范/验收摘要；监测读取路径，确认未读取reference/mutant或240答案材料。
2. 已有准备目录拒绝覆盖，原ready字节保持。
3. 未知槽在创建目标前拒绝。
4. Windows特殊路径、路径穿越和内容摘要不符拒绝。
5. 注入重解析点属性时拒绝；此项是受控检查，不宣称实机链接攻击矩阵已验收。
6. 注入写入失败时保留部分文件、无ready，再次准备同目录被拒绝。
7. 冻结清单摘要改变在创建目标前拒绝。

72槽已保存于`reports/efficiency-staging-241/prepared/`，另有一个刻意保留的`partial-failure/`，不要把它计作第73次实验。检查进程退出0，日志写句柄关闭已核实；0模型调用、0Host运行、0实验派发。未改Rust生产源码或0.1.15包，不追加构建、打包或全套回归。

证据：[结果](../reports/efficiency-staging-241/result.json)、[检查明细](../reports/efficiency-staging-241/validation-result.json)、[需求状态](../reports/efficiency-staging-241/requirements.json)。源码快照位于同目录`source/`，原路径/摘要见`source-manifest.json`。validate.py、run-checks.py和finalize.py均已完成，不重复运行。

## 单执行者基线核对与下一步

本轮核对的当前API路径表明：

- `api_host.rs`创建尝试时读取管理操作中的Assign决定，并持久关联management_operation/response_head；不能用一个人工构造的任务跳过此来源。
- `project_runner.rs`和`project_runner_parallel.rs`会启动Assign/Accept管理决策；单任务并不自动意味着零管理模型调用。
- `external_chief.rs`支持当前Codex会话提出方案，仍不替代批准，也不消除副指挥分配。

因此240合成管理的single-multifile场景仅是接线演练，不能当作真实单模型对照。上述源文件已归档，属于对已核路径的证据，不声称所有适配器不存在任何可用基线路线。

下一步先研究复用现有低层执行/权限/验收机制的单执行者路线，将基线身份与团队管理决策明确区分，不伪造副指挥来源、不放宽原管理授权、不建立另一套持续调度器。还需绑定原账本、终态检查和供应商用量；未知费用保持null。两模式的计时从各自正式任务开始，完整计入管理、人工与失败开销，不从团队墙钟里简单减去管理请求时间。

全部任务的分支独立验收接线、真实配置/预算、授权及72次真实实验仍未完成。现有准备目录不能代替上述门槛。238无需再发送，232保留原验收；正式签名、独立Windows、安装GUI及其他完整原范围门槛继续保留。账户实时额度仍不可可靠读取。
