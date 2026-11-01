# CeruleanLonging

**让每一步协作，有迹可循。**  

**Make every step of AI collaboration traceable.**

[简体中文](#zh) · [English](#en) · [界面预览 / Screenshots](#screenshots) · [项目反馈 / Feedback](#feedback)

发布者 / Publisher：**lantu**  

平台 / Platform：**Windows x64**  

阶段 / Stage：**公开预览 · Public Preview**


<a id="zh"></a>

## 简体中文

### 项目介绍

CeruleanLonging 是一款面向 Windows 的本地优先多模型协作工作台，将需求整理、方案规划、任务分配、编程执行、结果验证和交接记录组织在同一个项目流程中。

它面向需要持续推进实际交付的 AI 辅助开发场景：用户确定目标与边界，模型和工具承担不同职责，工作台记录过程、产物与检查结果。你可以围绕自己的项目目录组织工作，了解每项任务由谁执行、依据什么方案推进、结果是否经过检查，以及中断后还有哪些事项需要处理。

产品希望让 AI 协作成为可审阅、可接续的工作过程。无论是完成一个独立功能、修改已有代码，还是安排多个相关任务，需求、授权和交付依据都应能够串联起来。

### 产品特色

**分角色协作，围绕共同交付目标组织工作。** 主指挥负责方案与收口，副指挥负责分配与协调，执行者完成具体任务，独立验收工具检查结果。不同角色可以按受支持的接入方式配置，让规划、执行和验证各有明确职责。

**批准对应具体方案。** 项目需求、执行范围、产物和检查方式在执行前形成可审阅的配置与方案。用户能够了解即将授权的工作；配置或方案发生变化时，需要重新核对对应的执行依据。

**把验证放进交付流程。** 可以为任务登记独立检查工具和交付文件，也可以配置最终集成任务，对合并后的成果做整体检查。执行者的完成报告与实际检查结果分别记录，便于判断任务是否真正满足约定要求。

**检查方式可修订，原交付有据可查。** 当验收脚本或检查参数需要更正时，可以为原提交审阅并批准一次新的复验。原失败记录与交付文件保留，批准和启动分别操作；复验通过后仍需主指挥核对交付结果。

**实验有独立的工作空间。** 实验工作台使用单独的窗口和数据目录，方便试用配置或验证流程。凭证按工作台分别配置，创建实验本身不会派发模型任务。

**用本地工作流复用协作方法。** 可以导入带版本号的工作流模板，预览步骤依赖、完成标准与副指挥监督要点，再填入需求草稿。可按不同任务类型复用流程，模型、工具与执行权限仍由项目配置和批准方案决定。

**用项目记录延续上下文。** 计划、管理决定、任务尝试、检查结果和交接信息保存在本机。重新打开工作台后，可以继续查看原项目的记录，明确已完成的工作与待处理事项，减少跨轮次协作时重复整理背景的负担。

**异常状态也有记录。** 对失败、中断或结果未知的操作，工作台保留原记录，并提供相应的审阅和处置入口。恢复时先核对已有状态；未知请求不会被当作未发生而自动重新发送。

**兼顾日常使用与数据维护。** 桌面界面连接独立本地核心，项目记录的生命周期不依赖单个窗口。产品提供备份核对、数据恢复和升级维护入口，帮助用户在维护前了解当前状态并审阅变更。

### 主要功能

| 功能区域 | 可以用来做什么 |
| --- | --- |
| 工作总览 | 查看项目清单，进入任务、作业、运行记录、管理决定和恢复记录。 |
| 需求与计划 | 整理目标，配置角色、产物和验收方式，审阅方案并启动已批准的工作。 |
| 接入管理 | 管理模型服务和执行工具的配置，核对连接状态及可用能力。 |
| 执行与监督 | 查看任务进展、检查结果和需要关注的状态，暂停后续调度并处理异常。 |
| 复验审阅 | 针对原提交修订检查器，审阅并单独批准复验，核对原结果或保留证据结束复验。 |
| 实验工作台 | 在独立窗口与数据目录中组织实验，保留日常工作台的项目记录。 |
| 交接与历史 | 回看协作过程和决策依据，接续需要跨多轮处理的任务。 |
| 项目设置 | 管理项目目录、执行者、资源和整体交付检查等配置。 |
| 维护与升级 | 核对备份，审阅数据迁移与恢复操作，维护程序和项目数据。 |

### 一次典型的工作流程

1. **确定目标。** 关联项目目录，说明需求、交付范围与检查标准。

2. **配置协作方式。** 选择当前可用的模型与执行工具，设置适用的请求和执行限制。

3. **审阅计划。** 检查任务安排、产物与权限，批准本次方案并明确启动。

4. **跟进执行。** 查看分配、执行与验证记录；需要时暂停后续调度或补充信息。

5. **核对交付。** 结合独立检查和实际产物审阅结果，保留交接记录供后续继续。

例如，为现有项目增加一项数据处理功能时，可以先约定需要修改的文件与验证脚本，再由执行者完成实现，使用独立检查确认输出，最后回看整体交付记录。

### 接入与本地数据

项目覆盖模型 API、编程工具和桌面工具等不同接入方向，包括 Codex、OMP、Qoder、WorkBuddy，以及 OpenAI、Claude、DeepSeek、Qwen、Kimi、Grok 等模型服务相关路线。**各路线的角色支持、自动化能力和验证程度不同，列入范围不代表当前版本全部可用。** 请以当前版本的接入页面与说明为准。

本地优先意味着项目管理记录和工作状态主要保存在自己的电脑上。使用在线模型时，仍会按任务配置向对应服务发送请求及所需上下文，费用由相应服务计收；本地优先不等于全部离线。

暂停后续调度不等于撤销已发出的远端请求。请求次数或执行时限也不等于费用封顶。多模型协作的质量、用量和耗时取决于任务及配置，项目不承诺固定的 token 节省比例；角色协调与结果核对也可能增加用量和总耗时。

编程 API 的每轮输出长度可以按所选模型配置，实际输出仍受供应商上限和本次尝试剩余 token 预算约束。

金额预算配置目前可保存供审阅，尚不能用于启动执行。运行中异常监督按接入能力开放：Codex、OMP 和 API 的反馈有各自的确认状态；现有外部桌面接入暂不提供运行中严格监督保证。

### 实现概述

产品使用 Tauri、React / TypeScript 构建桌面界面，以独立 Rust 核心管理项目状态、执行流程与本地记录，通过适配层连接模型服务和执行工具。桌面与命令行共享核心能力，持久化记录支持历史追溯、状态核对与恢复。

### 当前阶段

**0.1.34 已完成本次约定范围的功能验收**，以**公开预览版**提供体验和反馈。验证结论限于已选配置和场景，延期接入及现场测试继续保留，详见 [支持范围](SUPPORT.md)。安装包尚无可信代码签名，本版本仍欢迎试用与反馈，不作稳定版承诺。

以上功能对应本次 0.1.34 发布包。旧内部预览附件保留为历史版本。

安装包与源码包见 [0.1.34 发布页](https://github.com/CereleanLonging/CeruleanLonging/releases/tag/v0.1.34)。自有代码采用 [MIT 许可证](LICENSE)，第三方组件保留各自许可证。

---

<a id="en"></a>

## English

### About the project

CeruleanLonging is a local-first, multi-model collaboration workbench for Windows. It brings requirements, planning, task assignment, coding, verification, and handoff records into a shared project workflow.

It is designed for AI-assisted development that needs to produce concrete deliverables over multiple sessions. You define the goal and boundaries, models and tools take on distinct responsibilities, and the workbench records actions, artifacts, and check results. Working with your own project directory, you can see who performed a task, which plan authorized it, whether its output was checked, and what needs attention after an interruption.

The aim is to make AI collaboration reviewable and resumable. Whether you are implementing a feature, changing existing code, or organizing related tasks, requirements, authorization, and delivery evidence should remain connected.

### What makes it distinctive

**Distinct roles working toward a shared deliverable.** A lead planner handles planning and final review, a coordinator assigns work, workers perform tasks, and independent verification tools check results. Supported integrations let you configure these roles with clear responsibilities for planning, execution, and verification.

**Approval tied to a specific plan.** Requirements, execution scope, artifacts, and checks form a reviewable configuration and plan before work starts. You can understand what you are authorizing; changes to the configuration or plan require the corresponding execution basis to be reviewed again.

**Verification built into delivery.** Register independent checks and expected files for tasks, and configure a final integration task to check the combined result. A worker's completion report and the actual verification results are recorded separately, helping you assess whether the agreed requirements were met.

**Revised checks with traceable original delivery.** If a validation script or its arguments need correction, you can review and approve a new check of the original submission. The original failure and delivered files are retained, approval and execution are separate actions, and a successful check still requires the lead planner's delivery review.

**Separate spaces for experiments.** Experiment workbenches use their own windows and data directories for trying configurations or validating workflows. Credentials are configured separately for each workbench, and creating an experiment does not dispatch a model task.

**Reusable collaboration through local workflows.** Import a versioned workflow template, preview its step dependencies, completion criteria, and coordinator guidance, then apply it to a requirements draft. Templates can support different task types; models, tools, and execution authority remain governed by the project configuration and approved plan.

**Project records that carry context forward.** Plans, management decisions, task attempts, check results, and handoff information are stored locally. Reopening the workbench lets you inspect the original project's records and distinguish completed work from outstanding issues, reducing the need to reconstruct context between sessions.

**Visible records of exceptional states.** Failed, interrupted, or unresolved operations retain their records and have corresponding review and resolution paths. Recovery starts by reconciling existing state; an unknown request is not automatically resent as if it never happened.

**Support for everyday work and data maintenance.** The desktop interface connects to an independent local core, so project records are not tied to the lifetime of one window. Backup verification, data recovery, and upgrade maintenance interfaces help you review the current state and proposed changes.

### Main features

| Area | What it helps you do |
| --- | --- |
| Overview | Browse projects and inspect tasks, jobs, runs, management decisions, and recovery records. |
| Requirements and plans | Define goals, roles, artifacts, and checks; review plans and start approved work. |
| Integrations | Configure model services and execution tools, and inspect connection status and capabilities. |
| Execution and supervision | Follow progress and check results, pause further scheduling, and resolve issues. |
| Recheck review | Revise checks for an original submission, approve a recheck separately, and reconcile its result or close it while retaining evidence. |
| Experiment workbenches | Organize experiments in separate windows and data directories while retaining everyday project records. |
| Handoffs and history | Review collaboration records and decisions, and continue work across sessions. |
| Project settings | Configure project directories, workers, resources, and final delivery checks. |
| Maintenance and upgrades | Verify backups and review migration, recovery, and program maintenance operations. |

### A typical workflow

1. **Define the goal.** Associate a project directory and describe the requirements, deliverables, and checks.

2. **Configure collaboration.** Choose available models and tools, and set applicable request and execution limits.

3. **Review the plan.** Inspect tasks, artifacts, and permissions, approve the plan, and explicitly start work.

4. **Follow execution.** Inspect assignment, execution, and verification records; pause further scheduling or provide input when needed.

5. **Review delivery.** Assess independent checks alongside the actual artifacts, and retain handoff records for future work.

For example, when adding a data-processing feature to an existing project, you can first agree on the affected files and validation script, let a worker implement the change, verify its output independently, and then review the overall delivery record.

### Integrations and local data

The project's integration scope includes model APIs, coding tools, and desktop tools: Codex, OMP, Qoder, WorkBuddy, and routes involving services such as OpenAI, Claude, DeepSeek, Qwen, Kimi, and Grok. **Supported roles, automation capabilities, and validation coverage vary by route. Inclusion in this scope does not mean that every route is available in the current version.** Refer to the current integration interface and documentation.

Local-first means that project management records and working state primarily reside on your computer. Online models still receive requests and the context required by the configured task, and the corresponding services charge for their usage. Local-first does not mean fully offline.

Pausing further scheduling does not revoke requests already sent to a remote service. Request counts and execution deadlines are not spending caps. Quality, usage, and elapsed time depend on the task and configuration; the project does not promise a fixed percentage of token savings. Role coordination and result review can also increase usage and total elapsed time.

Per-request output length for coding APIs can be configured for the selected model. Actual output remains subject to the provider’s limit and the remaining token budget for the attempt.

Currency-budget configurations can currently be saved for review but cannot start execution. Runtime supervision depends on the integration: Codex, OMP, and API feedback have distinct confirmation states; existing external desktop integrations do not yet provide a strict runtime-supervision guarantee.

### Implementation overview

The desktop interface uses Tauri and React / TypeScript. An independent Rust core manages project state, execution, and local records, while adapters connect model services and execution tools. The desktop and command-line interfaces share core capabilities, with persistent records supporting history, reconciliation, and recovery.

### Current stage

**0.1.34 has completed functional acceptance within the agreed scope** and is available as a **public preview**. Results apply to the selected configurations and scenarios; deferred integrations and on-site tests remain deferred. See [Support scope](SUPPORT.md). The installer does not yet have trusted code signing, and this preview is not a stable-release commitment.

The features above describe this 0.1.34 release. Earlier internal preview assets remain as historical versions.

Download the installer and source package from the [0.1.34 release](https://github.com/CereleanLonging/CeruleanLonging/releases/tag/v0.1.34). Original project code is licensed under [MIT](LICENSE); third-party components retain their own licenses.

---

<a id="screenshots"></a>

## 界面预览 / Screenshots

### 工作总览 / Project overview

![CeruleanLonging 0.1.34 工作总览 / Project overview](docs/images/overview-0.1.34.png)

0.1.34 当前候选安装版实测截图。展示项目入口与记录分类，图中的项目名和路径属于测试环境。

Captured from the installed 0.1.34 candidate test build. Shows project entry points and record categories; the project name and path belong to a test environment.

### 独立实验工作台 / Separate experiment workbench

![0.1.34 独立实验工作台 / Separate experiment workbench](docs/images/experiment-workbench-0.1.34.png)

0.1.34 当前候选安装版实测截图，展示实验工作台选择面板。示例目录和历史核查提示属于测试环境；创建与打开实验不会自动派发模型任务。

Captured from the installed 0.1.34 candidate test build, showing the experiment selector. Example directories and historical review notices belong to the test environment; creating or opening an experiment does not automatically dispatch model tasks.

### 整体交付验收 / Final integration checks

![整体交付验收配置 / Final integration check configuration](docs/images/integration-checks.png)

开发阶段配置界面截图，展示最终集成任务、交付文件和独立检查工具的关联；字段为测试示例，不代表该示例项目已经完成。

Development-stage configuration screenshot showing how a final integration task links deliverables to independent checks. Field values are test examples, not evidence of a completed project.

### 数据迁移与恢复 / Data migration and recovery

![数据迁移与恢复窗口 / Data migration and recovery window](docs/images/data-maintenance.png)

开发阶段原生维护窗口截图，展示备份核对与数据格式变更的审阅入口。界面包含开发中的能力，不代表所有费用控制与恢复场景已完成验收。

Development-stage native maintenance window showing backup verification and review controls for data-format changes. Some capabilities are still under development; the image does not establish acceptance of every cost-control or recovery scenario.

<a id="feedback"></a>

## 项目反馈 / Feedback

**项目反馈 QQ 群号：137562063**  

**Project feedback QQ group: 137562063**

欢迎反馈使用体验、功能建议和问题。描述问题时，请附上产品版本、复现步骤，以及去除敏感信息后的截图或错误信息；请勿提交 API 密钥、登录凭证或私人项目内容。

Feedback, feature suggestions, and bug reports are welcome. Include the product version, reproduction steps, and screenshots or error messages with sensitive information removed. Do not share API keys, login credentials, or private project content.

项目随缘更新，也欢迎提交 Issue；会在有空时集中查看和处理。

Updates happen as time allows. Issues are welcome, and I’ll review and address them in batches when I have time.

发布者 / Publisher：**lantu**

## Code signing policy / 代码签名政策

当前安装包未签名，项目现阶段不申请代码签名。发布状态与校验说明见 [代码签名政策](CODE_SIGNING_POLICY.md)，数据和外部服务使用见 [隐私说明](PRIVACY.md)。

The current installer is unsigned; the project is not pursuing code signing at this stage. See the [Code signing policy](CODE_SIGNING_POLICY.md) for release status and verification details, and the [Privacy notice](PRIVACY.md) for data and external-service use.
