# 247 单执行者实验入口与原来源回读

复用246单执行者模式、现有DurableWorkflow/API文件执行器/独立验收，以及242/243原来源导出。新增baseline_runner库和baseline_experiment独立CLI；普通RuntimeHost团队入口未改为基线调度器。

## 本轮能力

RunSpec固定具体数据目录、slot、完整PlanRevision和初始文件树；数据与项目目录不得重叠。prepare仅创建新数据根并提出方案，不批准或派发。run需要匹配整个spec的RunApproval，使用新root writer独占锁；开始意图先于凭证读取、工作树准备和任何请求落盘。半写意图也阻止续跑，缺失凭证保留失败终态。相同spec不能换数据目录复用批准。

执行共用原API/文件工具/请求与时间预算/验收/Accept，单调计时涵盖开始意图前至终态写入前（包括凭证准备、执行与验收）；不含CLI启动、人工审阅和终态落盘后的导出。失败/未知无重发。检查器有原批准的独立超时，未将这些时间伪装为完整用户端到端计时。

inspect只读原记录，不恢复或重跑工作流/检查器，也不读凭证；验证spec、计划、批准、终态与账本身份，复用原用量/验收文件读者。空管理原始目录明确保留并校验，出现管理记录即拒绝。

发现并修正来源读者的模式接线缺口：workflow_prefix原先固定用团队构造器；现按已有bindings选择246模式，再逐事件重放。普通团队原规则保持，旧218原导出回读等价（新的observedAt及Windows扩展路径前缀表述不同，原执行字段与时间保持）。

## 实际检查与失败保留

首轮9项：8通过，完整成功案例在回读全JSON比较时失败。该案例已Accepted，差异仅验收详情observedAt：它是本次读取时间，不是原执行时间。仅修正新测试比较，原产品代码未因该断言改变；原失败日志/程序/source.json保留，最终源码用corrected-source.json。

随后只运行1项原任务恢复检查：对原已完成案例只读回查及两次拒绝重执行，0新增派发；原data全树摘要前后、后续CLI回读后均相同。保留案例有2条合成执行者响应、0管理请求、1次真实独立检查、Accepted，单调耗时4200ms。原成功任务没有重跑，不能把首次失败报告改成全绿。

实际CLI覆盖6次：原终态回查、准备、准备回查、明确不存在凭证目录下的失败、重复调用拒绝、失败回查（详见result证据）。另1次旧218导出回读。不存在的凭证目录未创建，没有读用户Key或发送模型请求。失败CLI以退出码2返回原终态，已有意图重复执行以退出码1拒绝；两者均为预期结果。

旧218比较脚本还因输出路径的Windows扩展前缀差异停止一次，原导出命令本身已成功。核对两种路径的文件身份和原SHA256一致后，保存path-equivalence.json，仅续接未做的CLI检查，没有重导出218。首次比较失败脚本和回执保留，不能删除或称为产品失败。

fmt与core全目标严格Clippy通过，两个example构建并冻结。18个捕获进程均已终止且日志关闭；未重跑完整Rust套件、旧接入或原编程任务。原/修正测试程序与原导出程序都保留。build/checks/resume/finish-checks/finalize已经结束，勿重跑。

## CLI（需另有真实调用授权才能执行run）

```
baseline_experiment prepare <new-data-root> <spec.json> <spec-file-sha256>
baseline_experiment inspect <data-root> <spec-digest>
baseline_experiment run <data-root> <spec-digest> <approval.json> <approval-file-sha256> <credential-root>
```

JSON摘要先核对再解析；凭证只从明确指定的既有DPAPI目录加载，不复制到实验根。外部取消可由库调用方提供；本CLI没有新增交互取消界面。进程中断后只读原意图，不自动继续。新实验格式不作为普通团队项目导入或就地转换。

## 未闭合边界与下一步

RunSpec中的materialManifestSha256/configurationSha256目前是格式校验过的来源标记，尚未打开245材料逐项证明其任务/模型/工具/seed/checker与RunSpec一致。本轮是合成fixture，不能填入真实72槽；realityAttested和experimentRecordReady保持false。CLI是开发工具，RuntimeHost/UI、安装包均未接入这条入口。

下一步完成245→RunSpec内容级构造和核对，特别是相同worker参数/初始文件/验收器、独立Git目录及所有12任务分支；团队模式也要绑定同口径计时与启动前来源清单。之后冻结实际模型、预算及有限调用范围并取得真实授权。旧一次性授权/Key不复用。真实0/72；其余签名、安装GUI/升级恢复、独立Windows和供应商故障矩阵继续见requirements.json。

结果：[latest](../reports/baseline-runner-247/latest.json)。0真实模型/桌面消息、0用户凭证、0打包/安装/信任变更；0.1.15包清单保持且不含本轮源码。目标active，releaseAccepted=false。账户实时额度不可可靠读取，95%收尾规则保留。
