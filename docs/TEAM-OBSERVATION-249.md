# 249 团队 Host 的实验观察与来源保存

复用既有RuntimeHost持续调度、run-control、原账本、API工作树/文件工具/独立验收和242—244来源导出。新增受信任本机库入口与独立只读CLI；没有另写分配器，也未向UI/IPC增加启动命令。

## 行为

`RuntimeHost::prepare_team_experiment_inputs(project, expected_setup)`在规划前显式写入新的配置版本：快照只排除.git、100文件/1MiB，与248基线一致。原普通项目默认排除node_modules/target，若沿用会影响公平检查；本轮在测试前识别并补齐该入口。它要求没有方案/管理历史/运行，不更改已有批准，不改普通项目默认值。配置沿用现有不可覆盖版本记录与摘要；原模型/预算/权限保持。

`start_observed_team_run(project, revision, setup)`只允许隔离单项目、第一次已批准API/worktree方案、尚无执行记录且管理来源已处理的现场。核对完整初始树和实际Git原始提交，先保存started.json，再调用原start_project_run；原凭证/预算/批准/生产门禁全部保留。记录包含完整方案、批准、setup摘要、seed、起始账本头和管理来源库存。已有目录、半写或重复调用拒绝；launch.json保存原启动结果，发布不确定时阻止本Host继续自动调度。

返回的ObservedTeamRun保留同进程Instant；`finish(&host)`只能在原任务已非Active且所有自有工作静止时落终态，保存原run-control头、工作流头/任务状态、全部管理来源及单调耗时。结束时核对启动前管理记录仍逐项保留。管理规划虽在计时外，其用量来源仍留在初始库存及完整导出里，不被删除或记零。

`close_interrupted_team_observation`只封存已停止的原任务，不运行poll或恢复调度，耗时明确为null、clockContinuity=false。丢失计时token不能用墙上时间补成完整计时。开始记录尚无终态时只读返回started-without-terminal。底层Host已有恢复语义没有被观察器替换。

观察计时边界为已批准方案和预检之后、原运行启动/凭证检查之前，到调用方观察到静止的终态并收集最终库存之前，包含管理分配/收口、工作树、API、文件工具、验收和轮询等待。不含前期规划、人工审批、材料准备和事后证据导出，与基线执行段的边界接近，但不是完整用户端到端计时；完整规划及人工耗时仍缺，不能据此比较提效。调用方延迟finish也会计入观察耗时，后续实验驱动需固定轮询策略。

## 只读工具

```
inspect_team_run <data-root> <project> <start-digest>
```

在数据写锁下检查原始观察、批准/账本前缀、运行控制链、终态及管理来源；复用原API和验收文件读者，不打开RuntimeHost、不恢复或调度。新增RunLedger::read_existing，目录缺失直接报错，不能为核查创建新的空记录。完整材料绑定尚未接入团队，输出materialContentBound=false、realityAttested=false、experimentRecordReady=false。

## 实际检查

4组Rust检查通过，覆盖成功团队、启动缺凭证、管理失败、丢失计时token，以及未批准/错误摘要/初始内容变化/半写目录拒绝。5个隔离Host均使用合成管理/执行者；临时无效测试凭证已清理，无用户Key、真实模型或桌面操作。

成功案例走原Host三任务及集成：10条合成执行者响应、7条管理来源（其中1条规划在启动前）、3次独立检查、全部Accepted；两次Host重开没有重发。它使用既有集成夹具，不是239的真实效率槽，也不代证12任务全部运行。

4次原结果CLI回读前后目录内容一致。7份隔离损坏副本分别检验终态/计时字段不一致、管理库存改变、启动身份、缺失run-control、改动初始记录和半写终态；每份先证明调整数据目录后的完整副本可读，再损坏指定字段，全部拒绝。原数据没有修改。

复用244用量归一化器：17个来源、10执行/7管理，输入与输出各107均为合成已知小计；缓存未知、费用null、wholeRunUsageComplete=false保持。新夹具给不同任务唯一响应ID，避免把旧通用fixture重复ID当作真实重复请求。

fmt和core全目标严格Clippy通过，全部捕获进程结束、日志关闭。初版仅完成编译，在发现普通配置的快照差异后增加规划前入口，实际四组执行使用source-ready.json与core-tests-ready.exe。初版源码和二进制保留，但不算已完成的实机/实验。没有失败后重派成功任务。

## 下一步与正式边界

下一步将245团队材料的任务正文、相同worker配置、分解图、分支与整体检查绑定到实际Host setup/批准方案，复用248的固定来源校验和249的严格快照/观察。再补未覆盖的运行分支、统一规划/人工计时协议，冻结真实模型和单槽/总预算后取得具体有限授权。不能将这里的测试fixture或旧真实小任务放进72槽。

本轮没有把基线接入RuntimeHost/UI，也没有观察器的UI/IPC入口；0.1.15包保持且不含本轮新增源码。供应商/桌面完整故障矩阵、严格原任务监督、生产金额门禁、安装GUI/升级回退、独立Windows和正式签名仍见完整需求。真实效率0/72，releaseAccepted=false、目标active；当前无可靠实时账户额度，95%规则保留。

权威入口：[latest](../reports/team-observation-249/latest.json)、[结果](../reports/team-observation-249/result.json)、[需求](../reports/team-observation-249/requirements.json)。本阶段脚本均结束，不直接重跑或覆盖证据；238、232、OMP196、WorkBuddy203/206和218成果继续继承。
