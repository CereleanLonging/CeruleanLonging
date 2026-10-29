# 210 当前 Codex 会话进入正式 CLI / IPC

2026-10-20。已完成本机方案提交与恢复；正式发行验收仍未完成。

## 已接通的实际路径

工作台核心生成并持久保存上下文 → 当前 Codex 聊天通过本地工具读取 → 当前聊天提交结构化 Plan → 核心验证原请求、会话和当前状态 → 原工作流提出 revision → 等待独立用户批准。

本批使用真正编译的 `target/debug/cerulean-core.exe`，通过现有当前用户命名管道进入 RuntimeHost。没有创建替代 Codex 会话，没有调用新模型、派发执行者或批准方案。当前会话标识用于本地关联，不是供应商签名。工作台自动唤醒本聊天尚无已验证接口。

正式命令（打包后程序名为 cerulean-runtime.exe）：

```text
cerulean-core.exe --workbench --data-dir <数据目录> external-chief prepare --project <项目ID> --setup-digest <配置摘要> --session <当前会话标识>
cerulean-core.exe --workbench --data-dir <数据目录> external-chief read --operation <原操作ID>
cerulean-core.exe --workbench --data-dir <数据目录> external-chief submit --operation <原操作ID> --session <原会话标识> --file <完整ExternalChiefDecision.json>
```

成功响应的 `externalChiefResult` 为 true，`result` 含 `operationId/requestDigest/state/handoff/message`。使用核心提供的 requestDigest，不对漂亮打印的 JSON 文件自行计算摘要。提交文件包含 requestDigest、sessionId 和 envelope；envelope 包含原 stateDigest 及 Plan/Wait 决策。失败或结果未知先读取原操作，不能自动重发。提交返回 applied 表示方案进入工作流，不代表用户已批准或任务执行完成。

## 读取器与升级保护

管理 version2 外部来源证据要求 reader13。旧账本、原容器及原读取器记录不改写，不在启动时自动迁移。先按现有 upgrade review / prepare 锁定，再 upgrade backup 建立冷备份；依据真实返回的摘要调用：

```text
data-format review-management-source --from <备份> --manifest-digest <备份摘要> --hold-digest <锁定摘要>
data-format promote-management-source --from <备份> --manifest-digest <备份摘要> --hold-digest <锁定摘要> --review-digest <审阅摘要>
```

随后明确 upgrade release，再启动核心。迁移保留原事件和备份；缺保护的外部管理证据、未知读取器和被修改的来源标记拒绝读取。更新器识别 reader13，并拒绝读能力不足的 source/target。未实际安装或更新用户产品。

## 实际验证与修复

- 42 项定向 Rust 检查通过，5 项既有测试忽略；覆盖外部请求、真实 IPC、CLI 参数、读取器和备份恢复。209 的广泛管理回归继续引用，不重跑基础接入。
- 更新脚本的实际函数在受控包元数据下 8 项通过；未执行安装。
- 当前聊天通过正式 CLI 提交一次方案，applied；approval=null、0 次执行者尝试、0 次新供应商调用。正常重开及 reader13 冷备份恢复后原方案和管理账本一致。
- 冻结候选 0.1.12 的 reader12 核心已核对清单摘要和 build-info；实际拒绝读取 reader13，数据前后摘要不变。
- 真实 CLI 暴露 Windows 主线程 1 MiB 栈下的退出等待栈溢出。两次锁定均已写入，按原锁定继续，未重发。仅将异步任务放堆上不充分；最终使用固定 8 MiB 栈的命名 CLI 线程，等待线程返回，并保留堆上异步任务。真正的升级准备、退出等待、解除及重开已通过。原失败日志保留。
- 最终 fmt 和严格 core 全目标 Clippy 通过。格式修正只改变错误输出调用的换行，编译版本原字节快照与最终源码均保存，不为纯格式差异再构建。

证据：[汇总](../reports/management-tools-210/result.json)、[实机结果](../reports/management-tools-210/live/result.json)。本次隔离数据在 `reports/management-tools-210/live/data`，原恢复前数据在同级 `data-before-restore`。自己的核心已正常停止，用户应用未操作。

## 后续顺序与边界

1. 接配置中的明确管理来源、前端领取/回填与等待状态；现有模型名不代表副指挥已进入产品调度，旧 OpenAI 配置维持原默认语义。
2. 将208已验证的 DeepSeek Assign/Advise 适配器接入产品调度、计数/预算和恢复账本，保留真实供应商与 usage；不能伪造 OpenAI 回执。开发及受控测试不需要 Key。
3. 接外部主指挥最终 Accept，再按具体计划验收“当前聊天 Plan → 用户独立批准 → DeepSeek 分配/纠偏 → 执行与独立验证 → 当前聊天 Accept”。下一次真实调用需要新的明确授权；208一次性授权已消费，不能复用。
4. 完整首批矩阵、桌面原轮次严格监督、可靠性/效率、新候选安装升级及正式签名等门槛仍保留。207审计是当时结果，本轮没有重新审签或发布；不能将旧候选当作本批源码。

本次隔离配置的 workerModel 明确为 unverified-plan-only，没有验证任何新执行者型号；不得直接批准运行该演示方案。206恢复的原203任务已完成，不重发。账户额度没有可靠实时读数；用户历史29%不能当实时值，收到可靠已用95%读数后按既定规则安全收尾。
