# 230 实际结果：发送前停止，0输入、0发送，不自动重试

用户已批准原230计划执行一次。原冻结执行器在preflight的整页快照比对处失败；未写live-intent/task-review，未批准/派发、未输入/发送，项目仍空。自有进程退出101、日志关闭、剩余0。详见[实机结果](../reports/qoder-product-runner-230/live-result.json)。

后续只读回查中session/进程/模型/正文摘要/两条历史均保持，layoutDigest改变、节点数218→220。原230未保存触发失败的那一次观察，不能把后续快照等同于该次观察，也未能准确归因到具体控件。原尝试按约定停止；本地231修复在进行，不能再执行下方旧命令，旧审阅和程序仅作证据保留。

---

# 230 Qoder / Qwen 单次编程验收计划（原冻结范围）

状态：本地检查、受控产品演练及原聊天真实只读准备通过。尚未批准或发送编程任务；两条初始化授权已经消费，不能用于本次文件编辑。

## 已有成果与本批变化

继承228两条成功初始化、229真实历史边界修复、196 OMP、206 WorkBuddy及218管理闭环成果。本批不重复这些真实任务，不读取用户Key。Qoder依旧使用现有Token Plan配置。

新增Qoder专用prepare/once测试执行器与PowerShell自有Job入口，复用WorkBuddy的持久派发、独立产物检查与Accept流程。共享fixture增加“使用既有空项目”入口，不删除或重建已绑定的项目目录；原WorkBuddy入口不变。新代码仅属于测试/验收工具，没有新增生产功能。

执行器核对原session UUID、workspace、cwd、模型ID、版本、原两条历史及草稿/忙闲，固定Qoder.exe/app.asar、参考、测试程序、桥接、Node、只读会话读取器和验证器摘要。会话查询使用既有Node内置SQLite，只读选定聊天的四个元数据字段，不查询凭证或聊天正文。任何材料/身份变更均停止；唯一live-intent防止重复执行。

实际只读准备于2026-10-20 22:59完成：发现当前Qoder、原会话TaskPrepare、选定会话元数据、产品discover/inspect/bind/configure均通过；项目仍空、原两条历史保持，无approve/start/send。自有进程退出0、日志关闭、剩余进程0。

## 待批准的一次执行

- 聊天：隔离工作区“工作台 Qwen 验收 228”，session `0fa16906-4f98-4f59-8b4b-9ea6b556982a`。
- 模型：现有自定义 `Qwen-3.8-Flash, 中`，阿里云中国区Token Plan。界面/配置核对不代替供应商计费审计或模型真实性认证。
- 工作目录：`D:\test\CeruleanLonging\reports\qoder-product-228\project`。
- 最多发送1条编程任务，只请求创建`sum.mjs`：导出`sum(a,b)`，两个参数均为有限number时返回和，否则抛TypeError。
- 请求Qoder不运行命令、不联网、不读取凭证、不改其他文件；遇新增目录/工具授权时停止说明。外部应用权限不是单文件操作系统沙箱，不能声称技术上只允许写这一文件。
- 工作台独立运行已冻结Node验证器，15项检查后确认产物、最终Accept及两次重开无重发。命令由工作台检查器执行，非Qoder执行者。
- 任务策略90秒，原生回复等待50秒，自有执行器外层210秒；这些界限不是外部Qoder服务的费用硬上限，也不保证外部模型会停止运行。一条任务可能有多轮套餐计费推理。
- 不发送监督建议、不自动停止Qoder、不重发。失败或未知保存原意图，后续只读对账；不因读取超时重跑任务。
- 主、副指挥使用固定测试响应，不进行真实管理API调用。此项验收不能扩称Qoder全真实三级链通过。

固定审阅：`reports/qoder-product-228/review.json`。

批准摘要：`22e320f8d19d4d467d563d62a70de4d47e8ba0a2c945603b23c11e333613fa6f`。

固定测试程序：`reports/qoder-product-runner-230/core-tests.exe`，SHA256 `8d5628c8abc21b79fa4c471385a6b3ae06f1589213a91797a681ced21b9d2f1e`。

参考：`reports/qoder-product-runner-230/reference.json`。发送前仍重新只读核对，窗口重启/换聊天/材料变更不得沿用旧观察强行发送。用户可先最小化；获一次编程授权并恢复原窗口后再进入Live。

```powershell
.\scripts\test-qoder-product-live.ps1 -Live `
  -PreparedDirectory 'D:\test\CeruleanLonging\reports\qoder-product-228' `
  -EvidenceDirectory 'D:\test\CeruleanLonging\reports\qoder-product-runner-230\live-owned' `
  -TestExecutable 'D:\test\CeruleanLonging\reports\qoder-product-runner-230\core-tests.exe' `
  -ReferencePath 'D:\test\CeruleanLonging\reports\qoder-product-runner-230\reference.json' `
  -ApprovedDigest '22e320f8d19d4d467d563d62a70de4d47e8ba0a2c945603b23c11e333613fa6f'
```

仅在用户批准以上范围之后执行。上述输出目录必须新建，既有意图/结果不覆盖。

## 已完成检查与真实缺口

5项Rust保护检查通过、7项专项忽略不计通过；本批新增范围为目录保留/复用拒绝、隔离身份校验、安装包变更拒绝。6项SQLite读取检查通过；8项PowerShell路径保护检查通过，均ValidateOnly、0启动。受控Qoder编程流程通过15项产物检查和两次重开；fmt及核心all-targets严格Clippy通过。原聊天真实只读准备1项通过。

首次本地编译因误用了未声明的rusqlite依赖失败，改为复用既有Node内置SQLite，未更改Cargo依赖。路径检查第一轮日志解码异常，改为保留原始字节并断言具体拒绝原因，第二轮8项通过。原失败证据保留，不能算作真实模型失败。

完整证据见`reports/qoder-product-runner-230/result.json`、同目录源码快照/日志/回执、`reports/qoder-product-rehearsal-228-Q1ZYW4/result.json`。本批0新模型任务、0用户凭证读取、0打包/安装/信任变更。

正式验收未完成：仍需本次真实编程结果、其余供应商及严格监督/恢复矩阵、可靠性与效率证据、安装后GUI/替换回滚、独立Windows与正式签名。222冻结0.1.13尚未包含224与229生产修复。目标保持active；当前无可靠账户实时额度读数，95%规则继续有效。
