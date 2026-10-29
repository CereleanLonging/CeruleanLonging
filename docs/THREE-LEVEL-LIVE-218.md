# 218 修复后真实三级闭环计划（已准备，等待新授权）

2026-10-20T02:12:38.444215+08:00。当前217核心已接受本Codex聊天的新Plan，状态applied；用户批准null，0运行、0执行者尝试、0供应商调用、所有凭证为空。自有核心已正常停止。此计划和准备动作不构成真实执行授权。

## 为什么需要这一轮

216已接通DeepSeek并取得一次真实Assign回执，但模型返回适配器标识deepseek，未返回执行器标识worker-deepseek，核心正确拒绝派发。217已增加由批准方案生成的明确taskId/workerId绑定，保留严格身份检查和旧回执兼容。3项定向检查（含4种完整组合）、13项回归、fmt/Clippy/核心构建及原216数据只读重开通过。

本轮只验证修复后的真实分配、编程、独立检查和最终验收，不重复基础接通。216原失败记录不修改，原run不恢复，208与216一次性授权均已消费。

## 流程与固定范围

本聊天Plan → 用户批准固定方案 → DeepSeek副指挥一次Assign → 单个DeepSeek API编程执行者 → 固定Node验证器检查15个案例 → 本聊天检查实际产物及证据后提交Accept → 原位置重开核对，不重复派发。

- 新项目chief218，仅sum任务，无依赖及后续任务；文件为`D:\test\CeruleanLonging\reports\three-level-live-218\live\project\sum.mjs`。导出sum(a,b)，只接受有限原始number，其他输入抛TypeError。
- 副指挥deepseek-flash，最多1次Assign；执行者同模型但角色和账本独立，最多4轮请求、4次文件工具调用。每轮输出上限1024 tokens，执行者累计记录上限12000 tokens；单请求30秒、编程任务90秒。
- 管理账本最多3次：本聊天Plan、DeepSeek Assign、本聊天Accept；没有额外Advise额度。最多5次新供应商请求，可能产生多轮费用，调用/Token限制不等于精确金额上限。
- 执行者仅项目内读写工具，不运行命令、不访问外部资源、不改独立验证器。工作台仅为独立检查运行已固定Node验证器。模型请求使用内置DeepSeek官方端点，不额外做models probe。
- 失败、未知、身份/模型/文件范围不符或暂停即停止后续派发，不重发。账户可靠读数达到95%时不启动新任务或阶段并安全收尾；目前无可靠实时额度接口，不能保证自动阈值监控，不以Token或上下文占用替代。

## 已冻结的审阅对象

- 核心SHA256：`0c5c7ab2b7802f38a055ff4494f6bf04c5348fba4ed7851301afd42ee04931b5`；0.1.12 / reader13，源码与217已验证快照一致。
- 方案摘要：`b5da5ebbc4aa2b445af5020a3c1755b7c028aebe5ef8ce27fd36d4a995472843`。
- 配置摘要：`dbf3dc8f51f7ae4291489c5ec6da7afad85ff3e762c31a79a99f00787a25869a`。
- 本聊天会话：`01a0fc87-0ef5-75e3-ace2-53e577a16489`。
- Plan操作：`ui-plan-b7c421a9a106e31505294b9965f0f304-1`；请求摘要：`5436f99c1ba225dc1131290a8ff7391f3296ae50e598c5cffbabac6866510251`。
- 数据目录：`D:\test\CeruleanLonging\reports\three-level-live-218\live\data`。
- [真实方案](../reports/three-level-live-218/live/decision.json)、[配置](../reports/three-level-live-218/live/setup.json)、[准备后状态](../reports/three-level-live-218/live/prepared-project.stdout)、[217结果](../reports/assignment-contract-217/result.json)。

执行前核对摘要、当前批准为空、项目仍空及核心未改变；不因有历史授权自动批准，不重跑prepare_live.py或submit_current_plan.py。新数据必须留在原位置，reader13拒绝直接复制后搬移启动；217已保留该验证脚本失误和正确拒绝证据。

## 凭证与清理

需要本轮新的真实调用授权及本轮凭证使用授权。用户可明确授权仅本轮再次使用刚才提供的Key，无须重复发送Key；也可在本机配置并提供允许读取的工作台数据目录。没有新授权不读取/使用旧Key。仅匿名stdin或本机隐藏输入框进入正式credentials set，不写入脚本、命令行参数、报告或快照，不额外探测供应商。

如果授权使用来源目录，仅删除本次隔离数据中的临时凭证，不删除来源目录原有凭证。结束或失败后通过credentials remove删除本次临时副本，确认配置flag为空、临时DPAPI文件不存在，再正常停止自有核心。清理内存临时输入；不能声称删除聊天或供应商侧记录。

## 本轮不能替代的验收

真实异常监督、完整首批供应商与桌面矩阵、可靠性/效率及当前候选安装升级和签名仍需独立证据。本轮不安装、不发布、不创建Key/证书或变更信任；后续审签仍须按当时实际产物和其他模型改动核查。完整正式验收尚未完成。
