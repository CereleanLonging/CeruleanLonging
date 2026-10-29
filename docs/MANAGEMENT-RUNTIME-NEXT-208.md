# 208之后：把已验证的管理接通接入产品

当前事实：208已完成当前Codex真实方案提交与DeepSeek真实Advise调用。新增模块和隔离执行器可用，但RuntimeHost、运行器和界面仍走原OpenAI管理路径，不能宣布产品三级闭环通过。

用户本次一次性Key授权已消费；任何后续开发和受控测试都不能重用该Key。无需为了写代码重新获取Key；下一次真实请求必须具体化后取得新的授权。206已完成的203 WorkBuddy任务不得重发。

## 实施顺序和对应文件

1. 先在 `management_execution.rs` 定义来源明确的记录形态：保留旧OpenAI记录可读；外部chief记录会话/用途/操作/请求摘要，usage保持不可获取，不能写零或伪造响应ID；DeepSeek记录真实供应商、模型、usage、wire摘要。新记录需要版本/读取器门槛，先对照现有data_format与reader_generation再确定代数，不直接复用或降低旧保护。
2. 在 `host.rs` 的 `management_context` 与 `start_management_decision` 周围接入外部chief请求准备和回填：核心生成上下文和操作ID，持久化后返回等待；本会话工具读取请求并写回决策。核心验证来源绑定、请求未消费、最新配置/状态摘要以及原有budget/lease门槛。`management_ingress.rs`提供格式和绑定校验，不提供身份签名或用户批准。
3. 更新 `project_setup.rs` 主副指挥配置，以明确的来源枚举替代“模型名隐含供应商”。旧配置缺省仍映射旧OpenAI路线。副指挥DeepSeek调用复用 `deputy_management.rs`，credential_vault按供应商取Key；不得把DeepSeek回执包装成旧OpenAI记录。请求级限额可沿用；金额预算必须有对应供应商报价与usage覆盖，缺少时拒绝，不能静默退成不限额。
4. 更新 `project_planning.rs`、`project_runner.rs`、管理预检和监督入口：主指挥等待回填时显示可恢复的等待状态；缺OpenAI Key不应错误拒绝已配置的Codex+DeepSeek组合；副指挥只负责Assign/Advise。现有Apply/Approval和独立验证继续控制实际分配/收口。
5. 同步domain命令、TS契约、CLI/IPC与界面。外部chief回填不能通过模型文本直接触发Approve；“提交决策”和“用户批准具体revision”是两种不同权限操作。必须绑定当前project、operation与配置版本，不接受任意目录中的JSON当作已授权指令。

## 必要差异验证

- 保留已有OpenAI、桌面203/206、OMP196证据，不重做基础连接。
- 新增受控用例覆盖旧数据读取、重复/过期/跨项目/跨会话回填、用途越权、用户批准独立、崩溃恢复不重发、请求计数和供应商预算归属。
- 运行器真链验收需验证核心实际停在用户批准前；批准后副指挥分配；执行产物独立验证；当前chief收口。208的隔离执行器只证明接通，不是该验收的替代品。
- 当前会话可主动调用本地工具；工作台自动唤醒或直接控制这个Codex聊天仍无已验证接口。首版应明确采用“核心等待、本会话工具领取/提交”方式，不能暗中替换为新Codex CLI会话。

## 当前继续保留的正式验收缺口

完整首批矩阵、桌面原轮次严格建议、真实管理三级链、可靠性/效率、包含新源码的候选包及安装更新、独立Windows环境、正式发布证书。207当前实际签名审计有效，旧0.1.12候选不能代表新增源码；不新建密钥或证书、不变更信任、不发布。
