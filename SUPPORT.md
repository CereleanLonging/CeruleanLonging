# 0.1.38 接入与能力说明 / Integration and capability notes

[简体中文](#zh-support) · [English](#en-support)

0.1.38 为公开预览，新增由已保存 API 主指挥提炼 .md 文档、审阅确认后读入草稿，长文档分段处理。下表继承 0.1.34 已完成的约定范围实测记录，不表示全部供应商已在本版重测。0.1.35 引入的隔离与盲评使用本地合成回包及真实 Windows 账户验证；本版扫描只发现候选位置，登录和执行需分别确认，延期项保持原状态。

0.1.38 is a public preview adding .md refinement by the saved API chief, segmented long-document processing, and user confirmation before draft insertion. The table retains live-test records from the agreed 0.1.34 scope; it does not claim every provider was retested in this version. Isolation and review introduced in 0.1.35 use local synthetic responses and real Windows accounts. Discovery only lists candidate paths; login and execution require separate checks. Deferred items remain deferred.

<a id="zh-support"></a>

接入界面按主指挥、副指挥、执行端分区，不改变能力范围：主指挥支持 API 或绑定当前 Codex 聊天，副指挥支持 API，CLI/OMP 与 Qoder 桌面用于执行端。API 检查和 Key 管理分开；供应商 Key 在同一工作台内共享。模型变更仍须保存项目配置并审阅新方案。仅 Codex CLI 提供本机目录扫描。

The integration UI separates chief, deputy, and worker settings without extending capabilities: chiefs use API or a bound Codex chat, deputies use API, and CLI/OMP or Qoder desktop are worker routes. API checks and Key management are separate; provider credentials remain shared within a workbench. Model changes still require saved project settings and review of a new plan. Local program discovery is available for Codex CLI only.

## 简体中文

CeruleanLonging 面向 Windows x64，桌面界面需要可用的 Microsoft Edge WebView2 Runtime。接入能力取决于具体程序版本、服务地址、模型和账户权限。下表说明已经验证的配置与适用范围，不代表所有供应商组合均已验证。

| 接入 | 配置与能力范围 |
| --- | --- |
| Codex Agent / 当前聊天主指挥 | 已验证指定官方 Codex 0.166 配置的编程、修订、依赖集成和较长任务交付，以及当前聊天提交计划、核对并接受产物。工具执行与管理决策各自受批准范围约束。 |
| OMP | 已验证 OMP 18.4.4 配合 DeepSeek 的文件编程、修订、依赖集成及恢复。大数据双向通信与退出确认缺陷已修复并纳入当前安装包；0.1.34 的真实复杂任务修订通过独立检查、产物审阅、正常退出与重开恢复。原失败记录保留，结论限于已验证的配置和场景。 |
| DeepSeek API | 已验证官方 `api.deepseek.com` 的 `deepseek-flash` 编程、复杂任务修订、独立检查、本地取消及恢复。输出长度由配置、供应商限制和剩余任务预算共同约束。 |
| A6API 中转 GPT | 保留 `api.a6api.com/v1` 的历史证据；当前所选 `api.a6api.vip/v1`、`gpt-6.1-sol` 已验证文件工具调用、复杂编程的 Windows 兼容修订、独立检查、产物审阅及恢复。域名、模型或账户变化需分别验证，中转回复不能证明官方直连或上游身份。 |
| 官方 OpenAI API | 适配与本地协议检查保留，正式账户直连实测延期。Codex 和中转测试不代替这条路线。 |
| Claude API | 本轮按用户安排延期。 |
| Qwen API / 中转 | 本轮实际验证 `api.a6api.vip/v1`、`qwen3.8-flash` 的编程修订和本地取消恢复。此配置为第三方中转，不能代替官方百炼或 Token Plan 的实测。 |
| Kimi API | 本轮按用户安排延期。 |
| Grok API | 本轮按用户安排延期。 |
| Qoder / Qwen Token Plan 桌面 | 保留指定旧版本和界面配置的实验结果。应用升级、会话竞争和现场异常需要另行核对；界面模型标签不独立证明上游模型身份。 |
| WorkBuddy 桌面 | 已移除新接入与执行支持；旧记录只读保留，历史测试不代表当前支持。 |
| 统一工作台 CLI | 与桌面共用本地核心，提供项目配置、计划批准、执行、检查、恢复与维护入口。各供应商的实际能力以对应路线为准。 |
| 其他厂商原生 CLI | 除已列出的 Codex、OMP 外，本轮未新增其他原生 CLI 接入承诺。 |

### 新增隔离与盲评的适用范围

- 主指挥需求目录独立；副指挥经核心只读核对。执行总目录内最多五个身份目录，新身份才新建，已登记身份复用；执行工具受各自目录和已批准操作约束。
- CLI、桌面和新隔离任务的检查器需要绑定的普通 Windows 账户；账户建立可能需要 UAC。API 编程的文件访问仍由核心工具限定，客观检查也需检查账户。
- 项目盲评仅接受全名册无工具 API：主副指挥各一票，1—5 个执行身份各一票。同一执行身份去重；副指挥就绪确认不计票。Codex、OMP、桌面和外部当前聊天暂不能作为盲评席位，缺少支持时整轮拒绝。
- 用户先批准冻结材料和名册；各席只能收到核心提供的脱敏源码片段与批准的要求，不能调用文件工具或检索他人票据。完整收票后统一揭晓；全票后仍需用户确认同一交付版本。客观失败、缺票或技术错误均不能由多数票覆盖。
- 不通过后副指挥可提交一次失败分析，再由用户审阅进入既有主指挥问答与修订流程。重开不自动重发未决请求。
- 完整材料上限 96 KiB，最多 128 个源码文件，每片最多 2,048 UTF-8 字节；超限拒绝，不截断后宣称整库覆盖。最多 3—7 席、每轮席位数加 2 次请求，每次最多输出 4,096 tokens、90 秒，整轮 15 分钟；不是金额封顶。
- 启用后要求 reader19；同一数据目录不能继续使用旧宿主权限执行路径或新接入外部主指挥聊天。推荐先用独立实验工作台，升级前备份。旧版本不能读取新格式，恢复不会自动联网。
- 本机验证覆盖密封投票、全票与最终确认、恢复、票库/管道拒绝、子进程身份和检查目录权限回收。模型回包是合成数据，尚不能证明真实模型质量、供应商内部隔离或完全消除暗号。

### 使用时需要了解

- API Key 按服务地址和凭证类型配置。普通 API、第三方中转和订阅套餐凭证不能互换。
- 暂停后续调度与本地取消不能保证远端请求取消或停止计费；结果未知时保留原记录，不自动重发。
- 请求数、执行时限和 token 停止阈值均不是金额封顶。金额预算配置目前可审阅保存，尚不能用于启动执行。
- 外部桌面接入暂不提供原请求约束下的严格监督投送保证。工作台中明确受限的能力不能靠普通消息发送代替。
- 本地工作流模板可以预览并填入需求草稿；导入不会自动执行，也不会授予模型、工具或项目目录权限。
- 真实磁盘满、物理断电、全新系统及其他现场组合按本次范围保留延期，不作为已经实测通过的能力。
- 当前安装包未签名，现阶段不申请代码签名；见代码签名政策。

<a id="en-support"></a>

## English

CeruleanLonging targets Windows x64 and requires an available Microsoft Edge WebView2 Runtime for its desktop interface. Capabilities depend on the executable version, service address, model, and account permissions. The table describes validated configurations and their scope; it does not imply validation of every provider combination.

| Integration | Configuration and scope |
| --- | --- |
| Codex Agent / current-chat lead planner | The selected official Codex 0.166 configuration has been validated for coding, revisions, dependency integration, and longer task delivery. The current chat can submit plans and review and accept artifacts. Execution and management decisions retain their separate approval boundaries. |
| OMP | OMP 18.4.4 with DeepSeek has been validated for file-based coding, revisions, dependency integration, and recovery. Fixes for large bidirectional transfers and process-exit confirmation are included in the current installer. A live complex-task revision on 0.1.34 passed independent checks, artifact review, normal process exit, and reopening recovery. Earlier failures are retained; conclusions apply to the validated configuration and scenarios. |
| DeepSeek API | Official `api.deepseek.com` with `deepseek-flash` has been validated for coding, complex-task revisions, independent checks, local cancellation, and recovery. Output length follows the configured ceiling, provider limit, and remaining task budget. |
| GPT through A6API | Historical evidence for `api.a6api.com/v1` is retained. The selected `api.a6api.vip/v1` with `gpt-6.1-sol` has been validated for file-tool calls, a Windows compatibility correction to a complex coding task, independent checks, artifact review, and recovery. Domain, model, or account changes require separate validation; relay responses do not establish direct official access or upstream identity. |
| Official OpenAI API | The adapter and local protocol checks are retained; live validation with an official account is deferred. Codex and relay tests do not substitute for this route. |
| Claude API | Deferred for this acceptance scope at the user's request. |
| Qwen API / relay | This acceptance scope has validated coding revisions and local cancellation/recovery using `api.a6api.vip/v1` with `qwen3.8-flash`. This is a third-party relay, not validation of official DashScope or Token Plan access. |
| Kimi API | Deferred for this acceptance scope at the user's request. |
| Grok API | Deferred for this acceptance scope at the user's request. |
| Qoder / Qwen Token Plan desktop | Experimental results for selected older versions and UI configurations are retained. Upgrades, session competition, and on-site failures need separate checks. A displayed model label does not independently authenticate the upstream model. |
| WorkBuddy desktop | New integrations and execution support have been removed. Historical records remain readable; past tests do not imply current support. |
| Unified workbench CLI | Shares the local core with the desktop and provides project configuration, plan approval, execution, checks, recovery, and maintenance. Actual provider capabilities follow the corresponding route. |
| Other vendor-native CLIs | No additional native CLI integration is promised in this scope beyond the listed Codex and OMP integrations. |

### Scope of the new isolation and blind review

- Lead requirements live separately and the coordinator reads them through the core. The worker root holds at most five identity directories: new identities allocate a directory; registered identities reuse theirs. Tools follow directory and approved-operation boundaries.
- CLI, desktop, and checking processes for new isolated tasks require bound standard Windows accounts. Creating accounts may require UAC. API coding still uses core-scoped file tools, with an account required for objective checks.
- Every reviewer must support tool-free API review: one vote each for lead and coordinator, plus 1–5 distinct worker identities. Coordinator readiness is not a vote. Codex, OMP, desktop, and external current-chat roles cannot be reviewers in this version; an unsupported roster rejects the whole round.
- Users approve frozen material and the roster first. Reviewers receive core-provided redacted code fragments and approved requirements, with no file tools or access to other votes. Complete results are revealed together; unanimous approval still requires user confirmation of the same delivery. Objective failures, missing votes, and technical errors cannot be overruled by a majority.
- After rejection, the coordinator can submit one analysis for user review before the existing lead-planner question/revision flow. Reopening does not resend unresolved requests.
- Material is limited to 96 KiB, 128 source files, and 2,048 UTF-8 bytes per fragment. Oversized inputs are rejected, not silently truncated. Rounds have 3–7 seats and at most the seat count plus 2 requests, each capped at 4,096 output tokens and 90 seconds, with 15 minutes per round. These are not spending caps.
- Enabling review requires reader19. The same data directory cannot continue using legacy host-account execution paths or add an external current-chat lead. Use an experiment workbench first and back up before upgrading. Older versions cannot read the new format; restoring data does not initiate network calls.
- Local checks cover sealed votes, unanimity and final confirmation, recovery, private-store/pipe denials, child identity, and check-directory permission revocation. Model responses are synthetic, so these checks do not establish live-model quality, provider-internal isolation, or elimination of all covert signals.

### Before using an integration

- Configure API keys for their exact service address and credential type. Ordinary API, relay, and subscription credentials are not interchangeable.
- Pausing further scheduling and cancelling locally do not guarantee remote cancellation or an end to billing. Unknown outcomes retain their original records and are not automatically resent.
- Request counts, time limits, and token stop thresholds are not spending caps. Currency-budget configurations can currently be saved for review but cannot start execution.
- External desktop integrations do not currently guarantee strict supervision delivery bound to the original request. Sending an ordinary message does not substitute for a restricted capability.
- Local workflow templates can be previewed and applied to a requirements draft. Importing does not execute work or grant model, tool, or project-directory permissions.
- Real disk exhaustion, physical power loss, clean-system testing, and other on-site combinations retain their deferred status; they are not presented as tested passes.
- Current installers are unsigned; code signing is not being pursued at this stage. See the signing policy.

## 测量说明 / Measurement notes

固定 72 次效率对照完整记录了 50 次通过、22 次未通过；团队模式在这批测量中消耗的 token 和时间更多。原失败样本全部保留，项目不承诺固定节省比例，也不据这些结果断言一般质量等价。

The fixed 72-run efficiency study recorded 50 passes and 22 non-passes. Team mode used more tokens and time in this sample. Failed samples are retained; no fixed savings or general quality equivalence is claimed.

## 文档提炼 / Document refinement

导入只支持 UTF-8 `.md`；选择文档即可能发送给所配置 API 主指挥并计费。外部 Codex 聊天尚不支持后台自动提炼。用户确认前不会改写草稿，模型引用能定位不代表摘要完整正确；请对照原文审阅。文件和正文不设固定上限，但仍受本机资源、供应商能力及项目调用额度约束。保存大文档不代表模型一定能接受整个规划上下文。

Import supports UTF-8 `.md` only. Selection may send segments to the configured API chief and incur charges. External Codex chats cannot refine automatically in the background. Drafts remain unchanged until confirmation; locatable citations do not prove completeness or correctness. Review against the source. No fixed file/text import cap is imposed, but local resources, provider capabilities and project request budgets apply. Saving a large document does not guarantee the model can accept the complete planning context.
