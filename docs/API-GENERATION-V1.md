# API 生成与单次请求记录 V1

更新：第029项已将该传输接入核心库的批准执行、真实文件工具和产物提交，见 [API worker执行](API-WORKER-DISPATCH-V1.md)。尚未接入RuntimeHost常驻调度。内部消息上限现为256，以容纳获批的最多128轮；以下保留第028项实现和验证范围。

本批：2026-10-10，第028项。新增 worker API 传输和单次调用证据模块。尚未接入 RuntimeHost 的批准任务、文件工具、常驻调度或 UI，不能据此宣布供应商执行或 P3/P6 验收通过。

## 已实现

| 接入 | 请求协议 | 范围 |
| --- | --- | --- |
| OpenAI | Responses | 固定官方路由；store:false；自定义函数；输出及加密续接项回传 |
| Claude | Messages | 固定官方路由；原生 tool_use/tool_result；工具错误标记 |
| DeepSeek、Qwen、Kimi 中国站、Grok | Chat Completions | 固定官方路由；function/tool_calls；tool_call_id 关联；按需原样回传 reasoning_content |

Qwen 保留上一批的北京业务空间、新加坡、中国香港三种显式目标。八个端点变体均禁止自定义 URL、重定向、隐式代理和自动重试。指定模型 ID 原样发送，不把目录中的任意模型视为支持工具调用。模型兼容性和账户权限须后续逐模型实测。

每次最多 1 MiB 请求、2 MiB 响应、128 条内部消息、8 个工具定义、16 个返回调用；单段用户文本/工具输出/调用参数最多128 KiB。输出上限1–16384 tokens、单请求1–600秒；这些是本地上限，不是供应商模型的通用限制。供应商 usage 必须有效；没有价格时不推断费用为零。

只接受完整的成功结束状态；截断、拒绝、未知输出类型、未登记工具、重复调用 ID、无效参数 JSON、缺失用量、密钥原样或 JSON 转义回显都拒绝。错误不带远端错误正文。工具参数是否满足具体 schema、文件授权和副作用执行仍归可信宿主，本模块只输出调用提议。

私有协议续接留在内存中的 ApiReply；不能反序列化构造，不通过 IPC，不进入 Debug 或日志观察记录。OpenAI 加密推理项、Claude 签名思考块、兼容协议 reasoning_content 按协议续接。进程退出后不从普通观察记录重建续接，也不自动恢复发言。

## 单次记录及恢复含义

ApiCallJournal 绑定项目、任务、attempt、批准修订摘要、request ID、轮次、请求体和工具目录摘要。绑定不是批准证明，未来 RuntimeHost 必须另行检查当前用户批准、租约、剩余轮次/时间/费用预算。

一次发送顺序：

1. 核对 PreparedRequest 的指纹和新日志状态。
2. 独占文件锁下写入意图，sync，再以不覆盖的硬链接发布。
3. 只发送一次 HTTP POST。
4. 保存白名单响应观察或分类失败，发布成功后才向调用者释放 ApiReply。

两条记录构成哈希链。存在意图即禁止再次执行同一日志，包括重新打开后；成功回复只说明收到响应，不代表工具执行、产物提交或任务验收。网络错误、响应无效、超限及发送后本地取消均保守保留远端结果不确定。NotSent 是发送前停止；本地取消不能证明远端取消或免计费。

pending、缺失、额外、顺序错误、损坏和身份不符的记录拒绝打开。异常发布后当前对象失效；完整发布但尚未更新内存时，重开可读取已有记录而不会发送。此批未提供 pending 的用户复核恢复入口。哈希链不是防止有权限者重写或删除整个目录的机制；防竞态、断电及文件系统耐久性仍需专项验收。

## 验证范围

仅使用合成密钥和回环 HTTP，没有外部供应商 API 请求，没有真实模型调用。协议单元检查、80 个回环场景（8目标×10路径）、发送前取消不连接、日志执行次序、重开去重、丢弃运行中 Future、生产入口发送前取消、记录损坏及八个发布位置错误注入均纳入默认 Rust 测试。

八个发布位置为错误注入，不是实际进程终止或断电。原生界面未新增执行入口，本批不重复声明原生执行已验收。最终检查结果和可校验文件索引见 reports/p3-api-generation-review-20261010.json。

## 后续接线顺序

1. 为 worker 选择引入明确的 API 配置，绑定目标地域、模型、工具权限和预算至批准修订；不能借用 OMP 的 adapter 身份。
2. 在 RuntimeHost 建立 API attempt 的持久启动与管理分配关联，接入独立文件工具服务和每轮 CallJournal；先批准并记录工具意图，再执行工具，记录结果后才能发送下一轮。
3. 增加总轮次、累计时间、token/费用边界、主动停止、未知结果处置，以及与产物封存和独立验收的一致性验证。
4. 接入产品任务控制和流式展示；再做各家真实密钥的最小调用/工具回传验证。不能用目录成功替代执行成功。

统一工作台 CLI、厂商 CLI、Qoder/WorkBuddy 已有会话绑定、异构并行、效率测量、安装升级回滚等原验收要求继续保留。

## 核对过的官方资料

- OpenAI Docs：[Function calling](https://developers.openai.com/api/docs/guides/function-calling)，Responses 函数调用及返回结果。
- Claude：[Define tools](https://platform.claude.com/docs/en/agents-and-tools/tool-use/define-tools)，原生工具定义。
- DeepSeek：[Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode/)，工具对话中的 reasoning_content 回传。
- Qwen：[OpenAI 兼容 Chat](https://help.aliyun.com/zh/model-studio/qwen-api-via-openai-chat-completions)，地域路由与消息格式。
- Kimi：[思考模型](https://platform.kimi.com/docs/guide/use-thinking-models)，模型差异及上下文回传。
- xAI：[Generate Text](https://docs.x.ai/developers/model-capabilities/text/generate-text)，文本生成 API。

读取日期2026-10-10。官方协议支持与实际账号/模型兼容性验收分开记录。

## 本批最终检查

06:26 +08:00：341默认Rust通过/0失败/71忽略、5Node通过；fmt、严格clippy、核心/契约/前端/桌面debug构建通过，三处runtime SHA一致。新增13测试，包括80回环HTTP路径和8发布错误注入位置。无真实供应商请求/模型调用，无新增原生执行验收。详见第028项交接和reports/p3-api-generation-review-20261010.json。
