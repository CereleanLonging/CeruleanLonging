# 0.1.48 接入与能力说明 / Integration and capability notes

[简体中文](#zh-support) · [English](#en-support)

0.1.48 补齐逐执行端独立检查、失败返工、核心只读汇总和独立盲评之间的交接。每项成果合格并接受后才进入整体评审；全票与证据分达标后开放主指挥整体功能测试，最后由用户核对交付。检查器和盲评验收程序分别随工作台分发，固定关联并校验摘要。

Version 0.1.48 connects per-executor objective checks, correction of failed submissions, read-only delivery assembly and independent blind review. Every submission must pass and be accepted before whole-project review. Unanimity and evidence-score thresholds open the chief’s functional testing stage, followed by user confirmation. The objective checker and blind-review verifier ship as separately executed, hash-bound programs.

本轮 Codex 使用沿用当前账号的专用后台会话。已由桌面占用写入权的聊天需要先处理会话竞争，或选择专用后台会话。

The Codex experiment used a dedicated backend session with the current account. A chat held by an active desktop writer requires resolving writer ownership or choosing a dedicated backend session.

Markdown 导入使用内置本地提炼工具，按结构分段整理并保留原文行号，确认后读入草稿。管理员主窗口通过当前账户的普通权限导入窗口接收拖拽文件。独立实验台使用单独的数据目录和深色侧栏，日常工作台使用浅色侧栏。

Markdown import uses built-in local, segmented extraction with source-line references and confirmation before insertion. A standard-privilege import window supports file drops for the administrator main window. Experiment workbenches use separate data directories and a dark sidebar; the daily workbench uses a light sidebar.

桌面升级适配覆盖 Codex 程序重新定位和 Qoder 同协议版本验证。已选定且仍有效的 Codex 路径优先；原路径消失后可选唯一有效候选或唯一运行中 CLI，歧义时需明确选择。Qoder 0.4.3.0、0.4.8.0 有实测记录，后续 0.4 同协议候选需先通过本机固定消息验证，再重新绑定。其他协议系列需适配。版本、进程、路径、模型或桥摘要变化后需要重新核验。

Desktop upgrade handling covers Codex executable rediscovery and Qoder protocol-family validation. A valid explicit Codex path takes priority. A missing path can resolve to one unambiguous candidate or running CLI. Qoder 0.4.3.0 and 0.4.8.0 have actual test records. Later 0.4 candidates require a successful local fixed-message check and a fresh binding; other protocol families require adapter work. Changes to versions, processes, paths, models or bridge digests require renewed verification.

新检查材料启用 reader24，绑定 API 执行端评审输出额度的轮次启用 reader25。升级前请使用维护页备份，更新后的数据需由兼容阅读器打开。Windows GUI、核心与配套资源一起分发。编程项目默认目录为 `D:\test\coding`，可在项目设置调整。

New check material enables reader24. Rounds with configured API worker review output limits enable reader25. Back up through Maintenance before upgrading and use a compatible reader for updated data. The Windows GUI, runtime and resources are distributed together. Project registration defaults to `D:\test\coding`, adjustable in project settings.

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
| Qoder / Qwen Token Plan 桌面 | Qoder 0.4.8.0 的指定 BYOK 配置完成隔离账户编程、独立检查及六包盲评收发。界面所选模型为 Qwen-3.8-Flash（中）。发送期间需要保持目标窗口前台，焦点变化会停止当前操作并保留原记录。升级与接入配置变化须重新核对，模型身份依据该配置和界面标签记录。 |
| WorkBuddy 桌面 | 已移除新接入与执行支持；旧记录只读保留，历史测试不代表当前支持。 |
| 统一工作台 CLI | 与桌面共用本地核心，提供项目配置、计划批准、执行、检查、恢复与维护入口。各供应商的实际能力以对应路线为准。 |
| 其他厂商原生 CLI | 除已列出的 Codex、OMP 外，本轮未新增其他原生 CLI 接入承诺。 |

### 新增隔离与盲评的适用范围

- 主指挥需求目录独立；副指挥经核心只读核对。执行总目录内最多五个身份目录，新身份才新建，已登记身份复用；执行工具受各自目录和已批准操作约束。
- CLI、桌面和新隔离任务的检查器需要绑定的普通 Windows 账户；账户建立可能需要 UAC。API 编程的文件访问仍由核心工具限定，客观检查也需检查账户。
- 项目盲评支持无工具 API、受支持的 CLI 和已绑定桌面会话。主副指挥各一票，1—5 个去重执行身份各一票；副指挥先确认就绪。各通道均需提供完整的评分过程核验记录，工具调用或缺失证据会将该席判为未通过。
- 用户先批准冻结材料和名册。各席接收核心提供的脱敏源码片段、批准要求与匿名检查摘要，评分过程核验会拒绝工具调用。完整收票后统一揭晓，全票与证据分达标后开放主指挥整体功能测试，通过后由用户确认同一交付版本。客观失败、缺票及技术错误均保留具体原因。
- 不通过后副指挥可提交一次失败分析，再由用户审阅进入既有主指挥问答与修订流程。重开不自动重发未决请求。
- 完整材料上限 96 KiB，最多 128 个源码文件，每片最多 2,048 UTF-8 字节，超限时要求调整审阅范围。每轮 3—7 席，请求数按通道与实际材料计算并在批准前展示。Codex 分包载荷上限 6,000 字节，Qoder 为 1,500 字节，另有最终投票请求。API 执行端投票沿用该模型已批准的输出额度，同身份有多个任务时取其中最低额度，其他 API 席位和历史轮次默认 4,096 tokens。额度逐席展示并绑定本轮批准摘要。每个请求 90 秒，整轮 15 分钟，费用按所选服务计收。
- 新检查材料使用 reader24，绑定 API 执行端评审输出额度的轮次使用 reader25。执行模型使用分配的工作目录，评分接收批准的材料。恢复已有轮次先核对记录，再按该轮状态提供操作。
- 合成回归覆盖密封投票、全票与最终确认、恢复、票库和管道拒绝、子进程身份及检查目录权限回收。23 项真实 Windows 普通账户测试验证了检查隔离与权限回收。另以双模块合成项目完成真实服务实验，覆盖 Codex 专用后台会话、A6API 副指挥、DeepSeek 执行端和 Qoder BYOK 桌面，四席通过盲评后完成主指挥功能测试与测试交付确认。此前轮次中的人工桌面协助、一次旧核心受控终止及失败记录均纳入实验历史。证据分仍待校准，结论限于这些配置和场景。

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
| Qoder / Qwen Token Plan desktop | The selected Qoder 0.4.8.0 BYOK configuration completed isolated-account coding, independent checks and six-packet blind review. The UI model was Qwen-3.8-Flash (medium). Sending requires the target window to retain foreground focus. Focus changes stop the operation and preserve its records. Upgrades and configuration changes require renewed verification. Model identity is recorded from the selected configuration and displayed label. |
| WorkBuddy desktop | New integrations and execution support have been removed. Historical records remain readable; past tests do not imply current support. |
| Unified workbench CLI | Shares the local core with the desktop and provides project configuration, plan approval, execution, checks, recovery, and maintenance. Actual provider capabilities follow the corresponding route. |
| Other vendor-native CLIs | No additional native CLI integration is promised in this scope beyond the listed Codex and OMP integrations. |

### Scope of the new isolation and blind review

- Lead requirements live separately and the coordinator reads them through the core. The worker root holds at most five identity directories: new identities allocate a directory; registered identities reuse theirs. Tools follow directory and approved-operation boundaries.
- CLI, desktop, and checking processes for new isolated tasks require bound standard Windows accounts. Creating accounts may require UAC. API coding still uses core-scoped file tools, with an account required for objective checks.
- Blind review supports tool-free API requests, supported CLI transports and bound desktop sessions. The chief, deputy and 1–5 distinct executor identities each hold one vote, following deputy readiness. Each transport must supply a complete process audit; tool calls or missing evidence fail the corresponding seat.
- Users approve frozen material and the roster first. Reviewers receive core-provided redacted fragments, approved requirements and anonymous check facts. Process audits reject tool calls. Complete results are revealed together. Unanimity and evidence-score thresholds open chief functional testing, followed by user confirmation of the same delivery. Objective failures, missing votes and technical errors retain their specific causes.
- After rejection, the coordinator can submit one analysis for user review before the existing lead-planner question/revision flow. Reopening does not resend unresolved requests.
- Material is limited to 96 KiB, 128 source files and 2,048 UTF-8 bytes per fragment. Oversized inputs require a revised review scope. Rounds have 3–7 seats. Request counts depend on the transport and material and appear before approval. Codex packet payloads allow up to 6,000 bytes and Qoder up to 1,500 bytes, followed by a final voting request. API worker votes inherit the approved output limit, using the lowest limit when one identity spans multiple tasks. Other API seats and historical rounds default to 4,096 tokens. Per-seat limits appear before approval and are bound to the round digest. Requests have a 90-second timeout and rounds a 15-minute deadline. Provider charges apply.
- New check material uses reader24. Rounds with configured API executor review output limits use reader25. Executors use their assigned directories, and review receives approved material. Reopening a round first reconciles its records and exposes actions permitted by its recorded state.
- Synthetic regressions cover sealed votes, unanimity, final confirmation, recovery, private-store and pipe denials, child identity and permission revocation. Twenty-three real Windows standard-account tests validate checking isolation and permission recovery. A separate live-service experiment used a synthetic two-module project with a dedicated Codex backend session, an A6API deputy, DeepSeek and Qoder BYOK. Four passing votes opened chief functional testing and test delivery confirmation. Earlier rounds included manual desktop assistance and one controlled old-core termination. Failed attempts remain part of the history. Evidence weights await calibration, and conclusions apply to these configurations and scenarios.

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

文档导入支持 UTF-8 `.md`，由内置本地工具按结构分段提炼，保留原文与行号。预览中确认后读入需求草稿，后续请求主指挥时采用当前项目的接入配置。导入与提炼在本机完成，长文档处理受可用内存约束。规划请求仍需符合所选模型的上下文与项目调用额度。

Document import accepts UTF-8 `.md`. The built-in local tool extracts structured segments and preserves source text and line references. Confirmation in the preview applies the content to the requirements draft. A later planning request uses the current project connection. Import and extraction run locally, with long-document processing constrained by available memory. Planning requests must fit the selected model's context and project request budget.
