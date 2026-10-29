# 231 修复后的单次Qwen编程验收计划

状态：完整本地检查与原聊天真实只读prepare已通过，尚未发送编程任务。原230在发送前停止，0输入/0消息，原授权不自动重试；原记录全部保留。

本轮复用原“工作台 Qwen 验收 228”聊天和两条初始化，只更新Qoder批准到执行的布局校验，以及保留旧证据的新准备目录。修复依据与边界见[231修复说明](QODER-APPROVED-CONTEXT-231.md)。真实输入前的完整快照/历史/焦点检查保持不变。

## 待确认范围

- 使用现有阿里云中国区Token Plan，Qoder自定义`Qwen-3.8-Flash, 中`。原session为`0fa16906-4f98-4f59-8b4b-9ea6b556982a`。
- 最多1条编程消息，仅请求创建`D:\test\CeruleanLonging\reports\qoder-product-228\project\sum.mjs`。导出`sum(a,b)`，两参数均为有限number时返回和，否则抛出TypeError。
- Qoder不运行命令、不联网、不读凭证、不改其他文件；需要新增工具/目录批准时停止说明。外部应用沿用自身权限，不能声称存在单文件操作系统沙箱。
- 工作台单独运行已冻结的Node验证器，验证15项行为/文件条件，再检查最终Accept和两次重开无重发。
- 一个任务可能包含多轮套餐计费推理。任务策略90秒、原生回复等待50秒、自有执行器外层210秒；不等于外部模型费用硬上限，也不能保证外部模型随本地超时停止。
- 0条监督建议、0次自动停止、0次重发。失败或未知立即停止，保留原记录并只读对账；不再自动重试。
- 主、副指挥使用固定测试响应，无真实管理API请求；不能将此项宣称为Qoder全真实三级闭环。

## 已冻结材料

只读准备重新核对原session/cwd/model、程序摘要、草稿空/空闲、两条原历史；新产品配置尚无approve/run/task/live-intent，原项目仍空。新准备记录保存在独立目录，未删除或覆盖230原数据。

- 准备状态目录：`D:\test\CeruleanLonging\reports\qoder-context-231\prepared`。
- 审阅：该目录的`review.json`。
- 批准摘要：`dd58c806ee6bf456c77ebf541771de64e84b44ad70f0c67aafa4133291ec9653`。
- 执行程序：`reports/qoder-context-231/core-tests.exe`，SHA256 `2a90b625424a4080780297dc12855735097d6cd1e00fac3e19b4d4de8d621bb8`。
- 参考：`reports/qoder-context-231/reference.json`。安装包、Node、验证器、只读会话读取器、入口和自有Job代码摘要均在审阅中。

实际文件输出仍在228原项目，准备状态目录不同不代表另建聊天。发送前再次只读核对；不依据旧PID盲发，不在用户切换聊天或草稿非空时输入。

## 执行入口

仅获本计划的一次明确授权后执行，`live-owned`必须尚不存在。失败不重跑入口。

```powershell
.\scripts\test-qoder-product-live.ps1 -Live `
  -PreparedDirectory 'D:\test\CeruleanLonging\reports\qoder-context-231\prepared' `
  -ProjectDirectory 'D:\test\CeruleanLonging\reports\qoder-product-228\project' `
  -EvidenceDirectory 'D:\test\CeruleanLonging\reports\qoder-context-231\live-owned' `
  -TestExecutable 'D:\test\CeruleanLonging\reports\qoder-context-231\core-tests.exe' `
  -ReferencePath 'D:\test\CeruleanLonging\reports\qoder-context-231\reference.json' `
  -ApprovedDigest 'dd58c806ee6bf456c77ebf541771de64e84b44ad70f0c67aafa4133291ec9653'
```

## 当前验收状态

63项桌面Rust回归通过，25项专项忽略不计通过；11项路径检查、受控产物15项和两次重开、fmt/严格Clippy通过。新增3项上下文测试已包含在63项中，不重复计数。实际只读prepare通过，自有进程退出0、剩余0。

230零发送失败保留；231尚无新编程消息。证据见`reports/qoder-context-231/local-result.json`和`prepared-result.json`。正式验收未完成，222冻结包尚未包含224、229、231生产修复。当前无法可靠读取账户实时用量，95%收尾规则仍有效。
