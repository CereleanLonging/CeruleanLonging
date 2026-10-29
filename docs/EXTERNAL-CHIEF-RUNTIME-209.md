# 209 外部主指挥进入RuntimeHost同一决策账本

本轮承接208接通结果，推进真实产品核心接线。未使用Key、未调用模型、未操作第三方聊天；208的一次性授权已消费，仍不可复用。

## 已实现的核心入口

- `RuntimeHost::prepare_external_chief_plan(project, setup_digest, session)`：检查当前配置、项目空闲、请求上限和预算门槛，由核心生成上下文、操作号和状态摘要，持久化后返回待领取请求。没有OpenAI Key也可准备此类外部主指挥请求。
- `read_external_chief_request(operation)`：读取同一请求，不另起Codex会话、不触发模型。
- `submit_external_chief_decision(operation, session, decision)`：检查原会话关联、完整请求摘要、用途、当前状态及是否已消费，再写入回复。复用现有方案物化与应用事务，仅Propose，用户Approve仍为独立动作。

请求重开后保持`awaitingExternal`，不会变成可自动重发的API请求。用户可以放弃未应用的请求；Wait仍支持原有答复流程。半写回复需要明确复核；复核后的未知结果不能继续回填或重发。

## 持久证据与兼容范围

新增ExternalIntent/ExternalResponse存入同一`management-decisions`账本，继续使用原有临时文件同步、硬链接发布、应用意图和工作流回执。没有另建调度器或第二份项目真相。

新外部记录使用账本record version 2，旧OpenAI记录仍写version 1且不改写原摘要。旧核心的version 1检查及未知变体检查会拒绝这些新记录。当前尚未完成发行清单、全局数据读取代数和升级回滚对新记录格式的完整公布与验证；在开放正式CLI/界面及发行前必须补齐，不能拿旧候选包读取新数据来宣称兼容。

外部来源的`handoff`字段与旧计费wire request分开，避免被原reader4保护误识别。外部会话没有OpenAI API响应ID或可获取的API usage，不接受旧API回执，不列为旧OpenAI API欠费。会话/订阅成本仍未知，不能据此宣称零费用；配置金额预算的项目目前拒绝此入口，直到独立费用策略得到支持。

会话ID只证明本地关联，不能证明供应商远程模型身份；当前入口由受信任本机调用方使用，不把模型文本作为用户批准。

## 后续仍需完成

1. 对新格式补全数据保护、备份/更新清单与旧读者拒绝证据，然后通过既有当前用户IPC和工作台CLI开放领取/回填；同步界面等待状态及来源显示。
2. 配置主副指挥来源，接入DeepSeek副指挥的独立凭证、usage和预算路径；208适配器可复用，但尚未进入运行器自动分配/监督。
3. 接入当前外部chief的Accept收口以及运行器等待/唤醒；目前新增公开入口仅为Plan。
4. 验证独立用户批准后的完整三级产品闭环，继续保留首批矩阵、严格桌面监督、效率/可靠性、正式签名和独立安装环境门槛。

本轮具体结果以`reports/external-chief-runtime-209/result.json`及命令回执为准；测试和编译失败保留，不将修复后的通过结论回写到原失败记录。
