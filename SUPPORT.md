# 0.1.34 接入与能力说明 / Integration and capability notes

[简体中文](#zh-support) · [English](#en-support)

0.1.34 已完成本次约定范围的功能验收，当前阶段为公开预览。以下延期、受限和未签名项目保持实际状态。

0.1.34 has completed functional acceptance within the agreed scope and remains a public preview. Deferred, limited, and unsigned items retain their actual status below.

<a id="zh-support"></a>

## 简体中文

CeruleanLonging 面向 Windows x64，桌面界面需要可用的 Microsoft Edge WebView2 Runtime。接入能力取决于具体程序版本、服务地址、模型和账户权限。下表说明已经验证的配置与适用范围，不代表所有供应商组合均已验证。

| 接入 | 配置与能力范围 |
| --- | --- |
| Codex Agent / 当前聊天主指挥 | 已验证指定官方 Codex 0.166 配置的编程、修订、依赖集成和较长任务交付，以及当前聊天提交计划、核对并接受产物。工具执行与管理决策各自受批准范围约束。 |
| OMP | 已验证 OMP 18.4.4 配合 DeepSeek 的文件编程、修订、依赖集成及恢复。大数据双向通信与退出确认缺陷已修复并纳入当前安装包；当前包的真实复杂任务修订通过独立检查、产物审阅、正常退出与重开恢复。原失败记录保留，结论限于已验证的配置和场景。 |
| DeepSeek API | 已验证官方 `api.deepseek.com` 的 `deepseek-flash` 编程、复杂任务修订、独立检查、本地取消及恢复。输出长度由配置、供应商限制和剩余任务预算共同约束。 |
| A6API 中转 GPT | 保留 `api.a6api.com/v1` 的历史证据；当前所选 `api.a6api.vip/v1`、`gpt-6.1-sol` 已验证文件工具调用、复杂编程的 Windows 兼容修订、独立检查、产物审阅及恢复。域名、模型或账户变化需分别验证，中转回复不能证明官方直连或上游身份。 |
| 官方 OpenAI API | 适配与本地协议检查保留，正式账户直连实测延期。Codex 和中转测试不代替这条路线。 |
| Claude API | 本轮按用户安排延期。 |
| Qwen API / 中转 | 本轮实际验证 `api.a6api.vip/v1`、`qwen3.8-flash` 的编程修订和本地取消恢复。此配置为第三方中转，不能代替官方百炼或 Token Plan 的实测。 |
| Kimi API | 本轮按用户安排延期。 |
| Grok API | 本轮按用户安排延期。 |
| Qoder / Qwen Token Plan 桌面 | 保留指定旧版本和界面配置的实验结果。应用升级、会话竞争和现场异常需要另行核对；界面模型标签不独立证明上游模型身份。 |
| WorkBuddy 桌面 | 保留指定版本的连续交互、编程和只读恢复结果。桌面自动化依赖已绑定窗口与原会话，完整现场可靠性仍有边界。 |
| 统一工作台 CLI | 与桌面共用本地核心，提供项目配置、计划批准、执行、检查、恢复与维护入口。各供应商的实际能力以对应路线为准。 |
| 其他厂商原生 CLI | 除已列出的 Codex、OMP 外，本轮未新增其他原生 CLI 接入承诺。 |

### 使用时需要了解

- API Key 按服务地址和凭证类型配置。普通 API、第三方中转和订阅套餐凭证不能互换。
- 暂停后续调度与本地取消不能保证远端请求取消或停止计费；结果未知时保留原记录，不自动重发。
- 请求数、执行时限和 token 停止阈值均不是金额封顶。金额预算配置目前可审阅保存，尚不能用于启动执行。
- 外部桌面接入暂不提供原请求约束下的严格监督投送保证。工作台中明确受限的能力不能靠普通消息发送代替。
- 本地工作流模板可以预览并填入需求草稿；导入不会自动执行，也不会授予模型、工具或项目目录权限。
- 真实磁盘满、物理断电、全新系统及其他现场组合按本次范围保留延期，不作为已经实测通过的能力。
- 当前安装包尚无可信代码签名。签名申请与正式签名的状态将另行更新。

<a id="en-support"></a>

## English

CeruleanLonging targets Windows x64 and requires an available Microsoft Edge WebView2 Runtime for its desktop interface. Capabilities depend on the executable version, service address, model, and account permissions. The table describes validated configurations and their scope; it does not imply validation of every provider combination.

| Integration | Configuration and scope |
| --- | --- |
| Codex Agent / current-chat lead planner | The selected official Codex 0.166 configuration has been validated for coding, revisions, dependency integration, and longer task delivery. The current chat can submit plans and review and accept artifacts. Execution and management decisions retain their separate approval boundaries. |
| OMP | OMP 18.4.4 with DeepSeek has been validated for file-based coding, revisions, dependency integration, and recovery. Fixes for large bidirectional transfers and process-exit confirmation are included in the current installer. A live complex-task revision on this package passed independent checks, artifact review, normal process exit, and reopening recovery. Earlier failures are retained; conclusions apply to the validated configuration and scenarios. |
| DeepSeek API | Official `api.deepseek.com` with `deepseek-flash` has been validated for coding, complex-task revisions, independent checks, local cancellation, and recovery. Output length follows the configured ceiling, provider limit, and remaining task budget. |
| GPT through A6API | Historical evidence for `api.a6api.com/v1` is retained. The selected `api.a6api.vip/v1` with `gpt-6.1-sol` has been validated for file-tool calls, a Windows compatibility correction to a complex coding task, independent checks, artifact review, and recovery. Domain, model, or account changes require separate validation; relay responses do not establish direct official access or upstream identity. |
| Official OpenAI API | The adapter and local protocol checks are retained; live validation with an official account is deferred. Codex and relay tests do not substitute for this route. |
| Claude API | Deferred for this acceptance scope at the user's request. |
| Qwen API / relay | This acceptance scope has validated coding revisions and local cancellation/recovery using `api.a6api.vip/v1` with `qwen3.8-flash`. This is a third-party relay, not validation of official DashScope or Token Plan access. |
| Kimi API | Deferred for this acceptance scope at the user's request. |
| Grok API | Deferred for this acceptance scope at the user's request. |
| Qoder / Qwen Token Plan desktop | Experimental results for selected older versions and UI configurations are retained. Upgrades, session competition, and on-site failures need separate checks. A displayed model label does not independently authenticate the upstream model. |
| WorkBuddy desktop | Continuous interaction, coding, and read-only recovery results for selected versions are retained. Desktop automation depends on the bound window and original session; full on-site reliability remains limited. |
| Unified workbench CLI | Shares the local core with the desktop and provides project configuration, plan approval, execution, checks, recovery, and maintenance. Actual provider capabilities follow the corresponding route. |
| Other vendor-native CLIs | No additional native CLI integration is promised in this scope beyond the listed Codex and OMP integrations. |

### Before using an integration

- Configure API keys for their exact service address and credential type. Ordinary API, relay, and subscription credentials are not interchangeable.
- Pausing further scheduling and cancelling locally do not guarantee remote cancellation or an end to billing. Unknown outcomes retain their original records and are not automatically resent.
- Request counts, time limits, and token stop thresholds are not spending caps. Currency-budget configurations can currently be saved for review but cannot start execution.
- External desktop integrations do not currently guarantee strict supervision delivery bound to the original request. Sending an ordinary message does not substitute for a restricted capability.
- Local workflow templates can be previewed and applied to a requirements draft. Importing does not execute work or grant model, tool, or project-directory permissions.
- Real disk exhaustion, physical power loss, clean-system testing, and other on-site combinations retain their deferred status; they are not presented as tested passes.
- Current installers do not yet have trusted code signing. Application and actual signing status will be updated separately.

## 测量说明 / Measurement notes

固定 72 次效率对照完整记录了 50 次通过、22 次未通过；团队模式在这批测量中消耗的 token 和时间更多。原失败样本全部保留，项目不承诺固定节省比例，也不据这些结果断言一般质量等价。

The fixed 72-run efficiency study recorded 50 passes and 22 non-passes. Team mode used more tokens and time in this sample. Failed samples are retained; no fixed savings or general quality equivalence is claimed.
