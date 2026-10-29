# 196 OMP 最终Accept实机通过；一次性凭证已删除 / 2026-10-19T19:45:13.812726+08:00

用户新增一次性Key授权后，沿195固定方案完成一个真实OMP原任务和一条原消息建议；唯一sum.mjs独立检查通过，最终accepted，两次重开无重发，Job退出且剩余0。临时来源与隔离凭证均已删除，不得复用本次授权；平台Key与聊天记录未撤销/删除。193旧失败、195缺目录记录保留，以下“等待来源”“尚未实机重验”仅为历史状态。

桌面原生steer/buffer的12项安装源码合成检查通过，确定原请求绑定可行但没有消息ID去重、入缓冲不等于消费。现有桌面可访问传输、实际消息确认、恢复与WorkBuddy5.8.2适配仍待完成。真实OpenAI管理调用0，本次不代证完整三级闭环；releaseAccepted=false、目标active。

详见[196交接](../handoffs/2026-10-19/handoff_2026-10-19_003_194513+0800.md)、[结果](../reports/desktop-steer-196/result.json)、[凭证清理](../reports/desktop-steer-196/one-use-credential-cleanup.json)。本轮生产源码未变，未重新审签，旧成果与下方历史正文保留。

---

# 195 当前进度：前端版本检查已修复；OMP 来源目录缺失 / 2026-10-19T19:32:50.906391+08:00

15项受控UI、TypeScript/Vite及10项启动预检通过。WorkBuddy现安装5.8.2.0；9项安装队列类合成检查确认“立即发送”存在新prompt回退，不能直接用作严格原任务建议。旧版本接入成果保留，新版适配及桌面主动建议仍未完成。

用户已授权195实机差异及原来源，但一次入口因 D:\omp-live-credential-source 不存在而停在读取Key之前；0模型调用、无live-intent、未批准/启动，未自动重试。原因尚不能确定，旧保护索引未覆盖该外部目录，不能证明误删或未删。193失败与008修复保留，OMP最终Accept仍待实机核验。等待实际来源及用户继续指示，不自动替换凭证路径。

详见[195交接](../handoffs/2026-10-19/handoff_2026-10-19_002_193250+0800.md)、[结果](../reports/desktop-supervision-195/result.json)、[来源调查](../reports/desktop-supervision-195/credential-source-investigation.json)。7份命令回执已收尾，4个自有Job剩余0。本轮未重新审签；releaseAccepted=false、目标active。下方历史仅对应当时状态。

---

# OMP 收口等待顺序修复后的实机差异验证 / 195

状态：Prepare 已通过，尚未获本次 Live 授权；不读取凭证、不发送模型任务。原 193 的一次授权已执行并消耗，失败记录保留。本计划不是重新做基础接入，只验证修复后是否能从独立检查进入 Accept。

## 固定材料

- 准备目录：`D:\test\CeruleanLonging\reports\desktop-supervision-195\omp-fix-prepared-v2`
- 可审阅方案：该目录 `review.json`，摘要 `a7afad5eaaed7396b9f271e9057d42659e7467fec3eaae49f00720052aa33569`。
- 固定测试程序：`D:\test\CeruleanLonging\reports\desktop-supervision-195\prepared-binary\core-tests.exe`，SHA256 `62a77cd583272dcba5b9a9a3e6f9d23d632d0ffe376d7243f23719d310f22a32`。从194已测试程序复制并校验；以后重新编译不会覆盖这份材料。
- OMP：`D:\on my pi\omp-zh\omp-windows-x64.exe`，18.4.4，SHA256 `5644e5d7c7cddd851d79999dbc3b98363817178b33426b355a20fdcad41a3f54`。
- Node：`D:\qqagent\node\node.exe`，SHA256 `ba4e6d110e8c1592a1ecd390f6b05f3da124b13871a5be62b341a07a853c6c32`。
- 隔离运行父目录：`D:\test\omp-supervision-runtime-195`；新项目：准备目录下的 `project`。

## 待批准范围

DeepSeek `deepseek-flash`，最多一个原 prompt、最多一条监督建议、任务时限90秒；同一原任务通过批准文件工具创建 `project\sum.mjs`。刻意先读取两次不存在的文件触发监督，不将其宣传为自然卡循环。工作台独立 Node 验证五组数字输入，之后检查 Accept、只有一条原尝试、建议原消息关联，以及两次重开不重发。

一次原 prompt 可能包含多轮计费推理；没有精确 HTTP 次数或金额硬上限。文件工具的实际边界是整个新隔离项目，结束时额外检查只有 sum.mjs，并非单文件ACL。指挥响应仍为明确标注的合成响应，真实侧只有OMP/DeepSeek执行者，不代证真实OpenAI三级闭环。

建议使用上次用户指定的 `D:\omp-live-credential-source` 作为 DeepSeek 工作台凭证来源，**须本次明确允许后才能读取**。不扫描其他应用凭证；仅将 DeepSeek 凭证重新加密到隔离数据目录，执行后清除隔离副本，原来源不修改。无需向聊天提供 Key。

错误或未知立即停止，不自动重发；210秒外层私有Job界限，核对自有进程退出和日志关闭。新一次动作先写唯一 `live-intent.json`，该文件存在后绝不能重新发送。任何测试/OMP/Node/方案摘要变化需重新准备和核对，不能修改旧审阅文件。

## 执行入口（仅在收到上述范围新授权后）

```powershell
.\scripts\test-omp-supervision-live.ps1 -Live `
  -PreparedDirectory 'D:\test\CeruleanLonging\reports\desktop-supervision-195\omp-fix-prepared-v2' `
  -EvidenceDirectory '<新的 reports 子目录>' `
  -TestExecutable 'D:\test\CeruleanLonging\reports\desktop-supervision-195\prepared-binary\core-tests.exe' `
  -ApprovedDigest 'a7afad5eaaed7396b9f271e9057d42659e7467fec3eaae49f00720052aa33569' `
  -CredentialSource 'D:\omp-live-credential-source'
```

Prepare通过的实际回执在 `reports/desktop-supervision-195/omp-fix-prepare-v2.receipt.json`；首轮因运行父目录未创建而失败，尚未进入模型/凭证阶段，旧失败与退出回执保留。此次只补齐目录后在新准备目录完成；没有修改产品检查以求通过。
