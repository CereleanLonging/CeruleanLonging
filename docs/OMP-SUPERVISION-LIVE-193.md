# 唯一一次 Live 终态：协议/独立检查通过，Accept 闭环失败 / 2026-10-18T21:11:39+08:00

已按用户授权及指定 D:\omp-live-credential-source 执行一次，不重试。review摘要与测试/OMP/Node摘要匹配，保持原批准RuntimeParent D:\test\omp-supervision-runtime-193-v2（Live无该参数，未按消息中的不同目录替换方案）。live-intent先写，随后解密成功并进入发送。

原prompt completed，单条多行建议达到messageObserved；项目仅sum.mjs，独立Node五组检查exit0。但任务仍submitted/verified=true，run=attention、“OMP项目不可用”，未进入主指挥Accept，测试最终断言exit101，包装exit1。因此整体passed=false、realOmpSteeringAccepted=false。两次重开无重发，隔离凭证已移除，私有Job剩余0且日志关闭。realModelCalls=1仅指一个原prompt；providerCallCount=null，HTTP/推理轮次数未采集，不能声称一次HTTP调用。

本轮未改源码、未重建、未Git、未安装或改信任；旧制品除原脚本必然推进的prepared-v2/data/workflows/project/state.json外保持，原review与准备报告未改写。新证据有独立快照。详见[006交接](../handoffs/2026-10-18/handoff_2026-10-18_006_211139+0800.md)、[终态结果](../reports/omp-live-audit-193-20261018T130613Z/result.json)。goalStatus=active、batchAccepted=false、releaseAccepted=false。真实三级闭环/正式验收未通过；任何新发送须用户另行决定。以下历史正文逐字保留，不再沿用“等待凭证/尚未发送”作为当前状态。

---

# OMP 18.4.4 真实监督差异验证 / 193

本入口承接 191 的产品接线与 192 的旧读取器拒绝证据。原型基础接入不重跑，0.1.12 冻结安装包不修改。

## 本轮要解决的缺口

使用固定 `D:\on my pi\omp-zh\omp-windows-x64.exe`（18.4.4，SHA256 `5644e5d7c7cddd851d79999dbc3b98363817178b33426b355a20fdcad41a3f54`）核查多行 native steer 的队列文本、原 user 消息观察和编程产物。沿用产品原 Host、运行调度器、文件工具、管理应用、一次性投送许可与独立验收器，不另写调度器。

主指挥规划、副指挥分配/建议和主指挥接受使用明确标注的合成管理响应。真实侧是 OMP/DeepSeek 执行者。这是协议与产品接线的实机差异验证，不能作为真实 OpenAI 主副指挥闭环或正式验收。

## 可执行入口

- Rust：`crates/core/src/omp_supervision_live.rs`，仅在测试编译中存在。
- PowerShell：`scripts/test-omp-supervision-live.ps1`。
- Prepare：新建隔离项目/配置和未批准方案，固定执行器及 Node 验收器摘要，保存 `review.json` 与 `review-digest.json`。不启动 OMP，不访问凭证，不联网。
- Live：要求显式审阅摘要、同一测试程序摘要及明确授权的工作台凭证目录。原方案仍未批准且没有启动记录时才接受；先以不可覆盖方式写 `live-intent.json`，之后无论在哪一步失败均不能再次发送。
- 进程归属：复用既有 `OwnedRunner`，挂入私有 Windows Job 后才恢复测试进程；210 秒外层期限，退出和日志关闭单独保存回执。不操作用户已有的 OMP、Qoder、WorkBuddy 进程。

## 具体任务与副作用范围

1. 使用 `deepseek-flash`，派发最多一个原始 prompt。
2. 在新项目对 `sum.mjs` 连续进行两次完全相同的读取，文件尚不存在；这是刻意触发的监督信号，不用于宣称模型自然陷入循环。
3. 允许原管理链针对该异常投送最多一条多行建议，禁止重发，不放宽 queue/message 的严格关联检查。
4. 要求 OMP 通过原批准文件工具仅创建 `sum.mjs`，实现数值加法。文件工具强制边界是整个新隔离项目目录，并非单文件 ACL；结束时额外检查目录内只有该文件。无 shell、浏览器、其他目录或原生 OMP 工具权限。
5. 产品独立 Node 验收器检查正数、负数、零、小数、相消等五组输入。产物独立通过后才接受任务。
6. 核查原建议 `messageObserved`、只有一个执行尝试、任务独立验收、进程收回，以及两次重开不重发。各项单列，不以模型自述代替结果。

任务时间上限 90 秒，整段观察上限 165 秒，外层 210 秒。**一次 prompt 可以触发多轮供应商推理；当前不宣称严格 HTTP 请求数上限或金额硬上限。** 不应写成“只消耗一次 API 调用”。真实调用可能消耗额度，错误或未知停止，不自动重试。

## 凭证与执行前提

本轮准备不读取任何 Key。Live 只接受用户明确指定并授权的 CeruleanLonging DPAPI 数据根目录，仅读取其中 DeepSeek 凭证，重新加密保存到本次隔离数据根，正常或 Rust 展开退出后清除隔离副本；不改变原凭证。外层进程强制终止若留下加密副本，必须核查后仅清除本批隔离副本，不能宣称析构已执行。无需 OpenAI Key，不访问 OMP/其他应用凭证，不新建密钥或改变信任。

2026-10-18 本轮仅检查默认产品目录元数据：Roaming 下工作区存在，`credentials/deepseek.dpapi` 和 `openai.dpapi` 均未发现；Local 下同名 workspace 不存在。没有解密、读取内容或扫描其他应用；这不能证明用户其他配置目录没有 Key。真正执行前需明确实际来源，或由用户在工作台接入管理中配置；不要把 Key 发到聊天或命令行参数。

命令参数：

```powershell
# 先编译当前测试程序，取 cargo JSON compiler-artifact 给出的 executable。
.\scripts\test-omp-supervision-live.ps1 -Prepare `
  -PreparedDirectory <新的仓库reports目录> -EvidenceDirectory <另一个新reports目录> `
  -TestExecutable <编译产物完整路径> `
  -OmpExecutable 'D:\on my pi\omp-zh\omp-windows-x64.exe' `
  -RuntimeParent <新建且无祖先配置的隔离运行目录> `
  -NodeExecutable 'D:\qqagent\node\node.exe'

# 审阅后显式执行；不自动选择摘要或凭证来源。
.\scripts\test-omp-supervision-live.ps1 -Live `
  -PreparedDirectory <同一已准备目录> -EvidenceDirectory <新的实机命令回执目录> `
  -TestExecutable <同一测试程序> -ApprovedDigest <review-digest.json中的已确认值> `
  -CredentialSource <用户明确指定的工作台数据根目录>
```

后续若测试程序或批准方案变化，先重新准备并审阅，不复用旧摘要。已有 `live-intent.json` 的目录永不用于新派发；新一次真实尝试还须遵守用户失败停止/不自动重发的约定。

## 后续验收

本项真实观察通过后，再推进真实主副指挥完整业务链、桌面主动监督、首批接入能力矩阵、效率与可靠性、新候选及安装升级恢复。独立 Windows 和正式签名仍单列；本轮没有重新审签，不把旧未签名结论当作当前检查。原有完整验收范围不缩减。

## 当前准备结果与已获授权

准备入口已实际通过：`reports/omp-live-preparation-193/prepared-v2/review.json`；审阅摘要 `7258e452becdbcc902ab8d937835ee713e82d795f3ea6a4267635f23ba678b6d`。原测试程序为 `target/debug/deps/cerulean_core-311e0366bb05daa9.exe`，实际 SHA 固定在审阅文件；后续编译若替换其内容，旧摘要不再可用。运行隔离父目录为 `D:\test\omp-supervision-runtime-193-v2`。

本次最终测试编译、workspace/all-targets 严格 Clippy、fmt 检查通过；显式 Prepare 测试为 1 passed / 0 failed / 0 ignored，外层原进程退出码 0、剩余进程 0、两路日志关闭。首次准备在启动测试程序之前遇到 PowerShell 5 找不到 Get-FileHash；已改为 .NET SHA256，不依赖该模块，新目录重新准备通过。原失败目录和日志保留，不算作模型尝试，也未发生真实发送。准备辅助脚本一次 Python 命令引号错误未产出文件，随后通过独立脚本修正；未修改产品行为。

用户已回复 **“允许，配置工作台凭证后执行”**，授权上述范围的一次实际验证；不要再询问相同发送权限。当前等待用户在工作台「接入管理 → DeepSeek 执行模型 → DeepSeek API Key → 加密保存 DeepSeek Key」完成配置。默认来源为 `C:\Users\姚\AppData\Roaming\local.ceruleanlonging.workbench\workspace`；执行前只复查凭证是否出现，确认后按本次授权读取 DeepSeek 凭证。不得把此授权扩展到其他应用凭证、额外原始任务或失败重发。

准备成功不等于实机成功。当前没有 `live-intent.json`，真实调用为 0；`realOmpSteeringAccepted=false`、`releaseAccepted=false`。后续收到用户“已配置”或发现指定工作台凭证就位后，核对同一测试程序/方案摘要，执行一次 Live，并据原始结果继续；失败先读证据，不自动重发。没有缩减桌面、完整真实主副指挥、版本发布与正式验收的剩余范围。
