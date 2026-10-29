# 357：否决仅靠父目录固定锁文件的替代方案

356已证明metadata文件access0不能防止名称替换，安装器share0又与有效文件pin冲突。本轮针对另一条具体假设做一次新微型原生差异：数据目录用GENERIC_READ/share1固定，保留hold文件普通pin与writer.lock身份观察句柄，在父预检、NSIS等价打开、父后检间观察子文件操作。

本轮Windows 10 build19045结果：

| 检查 | 结果 |
|---|---|
| 父预检RW/share3、NSIS等价RW/share0、父后检RW/share3打开 | 全部成功，观察到相同writer.lock身份 |
| 持有严格父目录期间子文件路径rename | 拒绝，error32 |
| 同期将其他文件替换进子文件目标 | 拒绝，error32 |
| 同期DeleteFileW删除独立测试子文件 | **成功** |
| 事先持有DELETE句柄，再在父目录pin期间设置FileDispositionInfo | **成功**；关闭后文件消失 |

因此，父目录pin不构成“所有子文件名称不可替换”的保证。顺序交接三次打开相容不能代证交接安全，本轮不解除356的真实升级/恢复阻断。测试中的删除目标均为新fixture内独立文件，未删除真实writer.lock、宿主数据或旧证据。未运行真实NSIS、VM、模型、构建或产品代码。

[原始结果](../reports/hyperv-handoff-review-357/result.json)、[事件序列](../reports/hyperv-handoff-review-357/events.jsonl)、[脚本](../reports/hyperv-handoff-review-357/probe_parent_guard.py)保留。脚本exit0只表示实验完成，**不表示保护方案通过**；无未关闭句柄。357不解释328历史失败持有者，不否定356对目录自身名字保护的有限结论。

后续真正可评审的方向是让安装器和核心使用兼容的持续文件pin及同一字节锁协议：父级以非排他读句柄固定名字，父/子各自使用LockFileEx互斥。这个方向将涉及NSIS hook、指针/OVERLAPPED结构、失败释放以及新安装包，尚未实现或验证；既有0.1.21/0.1.24冻结安装器不能靠外部包装宣称已支持。尤其恢复源包仍使用旧share0协议，单改新目标不能关闭整条恢复门槛。不可将此设计意图写成修复通过，也不通过关闭检查/修改旧包或改变用户目录权限来绕过。

当前继续先补干净首装分支；完整升级/恢复缺口保留，正式验收未完成。
