# API 未知费用核对读取保护 / 184阶段14 / 开发中

API计数和生成费用的用户核销记录要求reader12。在原reader11数据停止核心、取得维护hold和冷备份后，使用 `--workbench --data-dir <原目录> data-format review-api-fee-resolution --from <原冷备份目录> --manifest-digest <原清单摘要> --hold-digest <原锁定摘要>` 审阅；核对后将子命令改为 `promote-api-fee-resolution` 并加 `--review-digest <本次审阅摘要>` 明确启用。原维护窗口有“API 未知费用核对”入口。

这只启用数据读取保护，不核销账单、不解除hold、不批准预算、不发送模型。失败先读取原状态，不能删除保护文件或重复提升。新reader12开发核心独立保存，尚不是统一发行候选；冻结旧核心与包不被覆盖。验收范围见[阶段14进度](../reports/api-currency-184/phase14-progress.json)。以下旧记录按阶段保留。

---

# API 逐轮费用读取保护 / 184 开发中

API 每轮计数、生成准备和费用确认记录要求 reader11。以原 reader10 数据的锁定及冷备份为依据，使用 `data-format review-api-currency --from <备份目录> --manifest-digest <原清单摘要> --hold-digest <原锁定摘要>` 审阅，再使用 `promote-api-currency`、同样参数及 `--review-digest <原审阅摘要>` 明确启用。命令前加 `--workbench --data-dir <原数据目录>`；维护窗口提供对应入口。

此操作只启用存储保护，保留锁定，不批准预算或发送模型请求。失败先读取原状态，不能删除保护文件或重发已有操作。运行验证仅使用本批隔离数据；当前发行包仍为 reader4，不能混入开发核心或脚本。正式发布前仍须统一版本与清单并完成剩余门槛。

---

# 服务档位和上下文报价保护 / 177 / 2026-10-16

新 schema2 报价、请求计价范围和实际响应档位必须在 reader8 下保存。停核心后取得原 reader7 数据的 hold 和冷备份，通过 `--workbench --data-dir <原目录> data-format review-scoped-pricing --from <备份> --manifest-digest <摘要> --hold-digest <摘要>` 审阅；核对后用 `promote-scoped-pricing` 及同样参数、`--review-digest <原审阅摘要>` 明确启用。维护窗口提供同样入口。重复启用拒绝，失败先回查原状态；不能删除保护文件伪造兼容。

启用只改变读取格式要求，不释放 hold、不授权金额、不启动模型。两容器的实际 CLI、隐藏维护窗口、更新互锁和旧备份恢复范围见 [177报告](../reports/scoped-ledger-177/result.json)。当前开发核心 reader8 仍为内部验证产物，不能与冻结 reader4 包混用；完整更新、安装版联合和正式发布仍需后续验收。

---

# 计数核销与终止续发 / 175 / 2026-10-16

计数费用和续发责任分开确认：费用核销只释放原计数预留并记入用户确认金额；终止续发不退款、不重置次数、不改变原结果。原用户工作流事件先持久化，账本后发布；重启仅恢复原确认，缺失/冲突或未发布碰撞保留现场。窄IPC与React支持分页、审阅、明确确认和原记录只读回查，失败不自动重发。已存在生成责任须先按原流程结清。

要求reader7：原reader6持有维护hold和有效冷备份，运行`data-format review-count-resolution --from <备份> --manifest-digest <摘要> --hold-digest <摘要>`，核对后同参数运行`promote-count-resolution`并增加`--review-digest <审阅摘要>`。维护窗口提供相同操作；启用不自动解除hold、不授权金额、不启动任务。仅本批隔离夹具执行了升级，用户目录未改变。

详见[175结果](../reports/count-resolution-175/result.json)。生产金额出口仍关闭，服务/上下文报价、其他管理角色/API轮次、完整故障与新发行候选尚未关闭。下方历史范围保留。

---

# reader6计数费用维护入口 / 174 / 2026-10-16

新增 `data-format review-counting --from <原冷备份目录> --manifest-digest <摘要> --hold-digest <摘要>`，以及 `data-format promote-counting` 加同样参数和 `--review-digest <原审阅摘要>`。由 `--workbench --data-dir <原目录>` 调用，仅支持原reader5数据。必须先停核心、取得hold并做冷备份，逐项审阅后明确发布；不自动解除hold、不授权预算、不启动模型。重做提升会拒绝，需读原status核对；不能删除旧/新保护文件。

更新控制器与维护窗口识别数字reader3–6，源/目标都须能读当前最低代际，reader6需有效原reader5。旧备份恢复保留原reader6数据归档。容器1/2实际CLI及受控隐藏窗口通过，详见[174结果](../reports/reader6-maintenance-174/result.json)；不是正式安装或完整断电验收。当前开发核心仍是0.1.10 reader6，不能混入冻结reader4包。

---

# 统一工作台 CLI

150 开发源码新增 `workspace execution-review --project <项目ID> [--digest <原执行方案摘要>]`，独立返回完整原方案、原用户批准及修订请求信息，不携带全项目任务证据或其他全局集合。首次读取可不带摘要；从已审阅方案回查时必须带原 `--digest`，方案变化即拒绝，不能把新的方案当成旧批准的结果。批准继续使用原 `plan approve` 命令及其原摘要；回执丢失时只读取原批准，不自动再次批准。此查询的摘要是原执行方案批准摘要，区别于 `workspace tasks` 的查询版本。当前新增接口尚未发行。

149 开发源码新增 `workspace tasks --project <项目ID>`、`workspace task --project <项目ID> --task <任务ID> --revision <任务页版本>`、`workspace task-evidence --project <项目ID> --task <任务ID> --revision <任务页版本>`。任务和证据列表用 `--offset <nextOffset>` 翻页，续页保留同一 `--revision`。这是选中项目原状态的查询版本，不能拿来批准方案；状态变化后须刷新任务首页。任务详情单独读取，证据引用单独分页，引用本身不代表验收通过。协调器中断、原项目缺失或处于诊断模式时明确返回不可用，不自动修补记录或开始执行。新命令尚未进入冻结发行版。

2026-10-14 / 148 开发源码更新（未发行）：修改命令成功后返回 `commandReceipt.requestId`，与外层 `requestId` 相同，`result` 为 `null`。这是本地命令已被核心接受的回执，后台完成状态须单独读取。原 `status` 等查询仍返回完整结果；大工作区可用 `workspace overview`、`workspace plans`、`workspace projects` 和 `workspace plan --id <原ID> --revision <列表版本>` 独立查询，结果用 `workspaceResult: true` 标识。翻页参数为 `--offset <nextOffset>`、`--revision <原列表版本>`；必须沿用服务端游标，不能自行假设每页固定数量。完整快照超限或通信丢失时不得重发修改；按原项目、任务、尝试或批准身份读取原记录。回执没有单独持久化日志，不能按标题推断一次创建的结果，也不构成跨重启的通用去重保证。冻结旧版本的输出保持原样，自动化脚本使用本轮开发核心时须读取新的回执字段。

2026-10-12 / 093。CLI 已通过隔离本机服务与受控模型编程闭环验证；真实六家供应商及完整 P3/P6 仍须按原方案验收。

CLI 位于已经随桌面打包的 `cerulean-runtime.exe`，入口为 `--workbench`。它连接桌面使用的同一用户命名管道，不创建另一套调度、批准或账本。开发目录也可用 `target/debug/cerulean-core.exe`。

094 新增离线维护入口 `backup create|verify|restore`，直接使用原数据目录的单写锁，不连接或启动核心。支持校验后的同用户、原位置恢复，目标必须不存在；完整用法和范围见 [冷备份说明](COLD-BACKUP-V1.md)。以下项目操作仍使用原 IPC。

096 开发构建新增 `upgrade review|prepare|status|backup|release`，支持状态摘要审阅、持久锁定、绑定备份与明确解除。完整顺序和旧版本边界见 [升级准备说明](UPGRADE-PREPARATION-V1.md)。095 冻结安装包尚未包含这些新命令。

```powershell
cd D:\test\CeruleanLonging
.\target\debug\cerulean-runtime.exe --workbench --help
$data = "$env:APPDATA\local.ceruleanlonging.workbench\workspace"
.\target\debug\cerulean-runtime.exe --workbench --data-dir $data service start
.\target\debug\cerulean-runtime.exe --workbench --data-dir $data status
.\target\debug\cerulean-runtime.exe --workbench --data-dir $data providers list
```

应先以桌面设置里显示的实际核心数据目录核对 `$data`。`service start` 连接已有核心或启动当前程序同版本核心；其他命令不会自动启动。服务只有在无未决任务时才接受 `service stop`。关闭 CLI 不停止后台任务。

## 配置、审阅与执行

`project add --id <ID> --root <项目目录> --timezone Asia/Shanghai` 登记项目。`validator add --file validator.json` 登记独立验收器，文件字段为 `id/name/executable/inputs/environment`，后两项可省略；程序和输入摘要由原核心观察并固定。

`setup save --file setup.json` 读取原 `ProjectSetupDraft` 契约，支持 UTF-8 BOM，最大60 KiB。契约见 [Rust 定义](../crates/domain/src/project_setup.rs) 和 [前端生成类型](../apps/desktop/src/contracts.ts)。API 执行配置与工作目录、资源、验收器、工具及预算都沿用桌面字段。保存配置不会批准、派发或调用模型。未知字段、错误类型和超限输入在发送前拒绝。

顺序如下，每个摘要都应来自用户已经审阅的当前记录；下面尖括号为需替换的值。

```text
setup show --project <ID>
plan request --project <ID> --setup-digest <当前配置摘要>
plan show --project <ID>
plan approve --project <ID> --digest <workflow.revisionDigest>
run start --project <ID> --revision-digest <已批准方案摘要> --setup-digest <配置摘要>
status
```

这些命令均接在 `cerulean-runtime.exe --workbench --data-dir <目录>` 后。`plan request` 和 `run start/resume` 可能产生模型费用，返回成功表示核心接受请求，不表示后台模型或任务已经完成。用 `status` 读取 `projectRuns`、工作流、各执行者及独立验收状态。`plan show` 返回完整原方案、权限、批准和配置，没有自动选取最新摘要或自动确认。

## 暂停、对账与检查

- `run pause --run <ID>` 停止后续调度，继续收集已经发送的任务。`run resume` 使用与 start 相同的显式项目、方案和配置摘要，并受原未决状态关口约束。
- `task recover --adapter codex|omp|api|desktop --project <ID> --attempt <原尝试ID>` 映射原对账命令，不重建或重发任务。是否允许恢复由原核心证据决定。
- `task cancel` 使用相同定位参数并加 `--reason`；Codex、desktop 另需当前停止审阅的 `--evidence-digest`。当前摘要可从完整 `status` 原审阅记录获取。
- `management list/show/answer/discard` 使用原分页、请求、答复和处置摘要；不引入其他角色权限。
- `verification show/recover/cancel` 使用原项目和验收 `operationId`；recover 还需当前证据摘要，不将执行器自报成功当作独立验收。

完整参数见 `--help`。本批实机 CLI 验证覆盖实际批准、配置、运行、独立验收、详情、停止与重开；暂停、各执行者取消和恢复的底层证据继承原验收，本批没有逐执行者重做真实模型或桌面动作。

## 凭证与输出

支持 `openai/anthropic/deepseek/qwen/kimi/grok` 六个原凭证槽。`credentials set --provider <标识>` 只从标准输入读取16–512个可见 ASCII 字符，允许末尾一次 LF/CRLF；不接受命令行 `--key` 参数。标准输入用于可信凭证供给程序，不是隐藏输入窗口；交互录入可继续使用桌面密码框。输入、错误、结果和日志不回显 Key。`credentials remove` 只删除指定原凭证槽。

`providers list` 是本地状态；`providers check --provider openai` 或 `providers check --file endpoint.json` 才请求官方模型目录。后者使用原闭合 `ProviderEndpoint` 契约，不能传任意 URL。凭证配置成功、模型目录成功与生成/工具能力通过是不同结论。

成功在 stdout 输出 JSON（`ok/requestId/serverPid/stopping/result`），失败在 stderr 输出 JSON。退出码0表示请求成功，2表示输入错误，3表示核心拒绝，4表示连接或通信失败，5表示结果输出失败。`resultUnknown` 意味着请求可能已入账；`outputFailed` 意味着核心已响应但本地输出失败。均不可据此自动重发，应查询原状态。CLI 每次最多发送一个用户命令。

## 验证与修复记录

复用原本机身份/ACL、单实例启动、版本化 IPC、全方案批准、API配置、管理决策、文件工具和独立验收。新增真实 CLI 检查包括含空格中文路径、六个合成凭证槽、八条 API 配置路线、保存零派发、未批准/过期摘要拒绝、两项依赖编程任务与独立验收，以及停止重开原事件不变。模型与主副指挥回复为原测试夹具，实际文件、核心进程、CLI、命名管道和验收器为真实运行。

首轮验收脚本误读验证记录的 `id`，改为契约中的 `operationId`。补上子进程超时检查后发现常驻核心继承 CLI 输出管道：虽然请求已返回，调用者仍等不到 EOF。启动现使用原生隐藏进程且不继承句柄，仍保留原管道 PID/SID核验、并发竞争失败者清理与既有锁。原失败日志保留，不能把首次忽略超时的基本流程结果作为最终通过证据。

构建、检查及冻结报告以 [093索引](../reports/p3-workbench-cli-20261012.json) 为准。完整 P3/P4/P5/P6 未放行；原桌面会话接入要求、真实主副指挥和全部真实供应商、真实效率及安装迁移回滚签名仍保留。
