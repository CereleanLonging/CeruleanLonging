# 245 实验材料持久化入口：受控检查通过

本轮继承239固定任务集、241初始工作区、242/243原记录读者及244用量归一化。238单次实机授权已消费且通过，无重发，Qoder无需前台。原包0.1.15保持，正式验收仍未完成。

## 改动与依据

新增 `scripts/efficiency-run-bundle.py`：freeze固定seed/任务/验收规范与不含凭证字段的配置；attach采用独占创建的意图、原文和回执；review核对调用方指定的manifest SHA256、固定任务集和附件。配置仅是材料侧限制检查，不能替代Rust运行时校验。

接手时的未验证脚本已保存为 `reports/efficiency-bundle-245/initial-efficiency-run-bundle.py`。此次补齐来源pin集合完整性、清单与固定任务集的一致性、文件/目录清单、链接拒绝、布尔值冒充整数拒绝，以及附件重读时的范围检查。现有241目录和旧冻结报告未改写。

## 实际验证

21项受控检查通过，freeze/review/attach三次CLI通过；四个捕获进程均退出，日志关闭经独占读取核实。包括同模型配置跨模式一致、错误配置/seed/规范/来源拒绝、额外文件与空目录拒绝、NTFS junction/hardlink拒绝、覆盖拒绝、两写者竞争仅一个成功、部分失败保留且拒绝续写、错误槽/摘要/项目/版本拒绝、原218/240导出不得归入新实验。

所有附件齐全的合成案例仍返回 authorized=false、actualExecutionKind=null、evidenceContentAuthenticated=false、experimentRecordReady=false。不能将本轮合成配置用于用户模型调用。

结果：`reports/efficiency-bundle-245/result.json`；需求表：`reports/efficiency-bundle-245/requirements.json`；源码快照及摘要：`reports/efficiency-bundle-245/source.json`。测试中的部分失败和链接夹具有意保留；不要递归复制这些夹具或将其计入实验。

## 使用与边界

freeze需要新的目标目录；返回manifestSha256。attach和review必须提供该摘要（`--expected-manifest-sha256`），附件另需 `--expected-sha256`。摘要应来自调用方保存的记录，不能在发现变动后自动换成当前值继续。失败材料不覆盖，不自动重试。

这是材料入口，不是执行器或授权机构。可拒绝保留原身份的旧218/240导出，但不能认证人为改名/伪造的导出；同一账户恶意并发修改文件也不在当前保证内。材料中的时间和授权说明没有真实性保证，缺少运行时计划/任务/seed/工作树、原始调用前库存和完整单调计时绑定。既有审批、角色和金额门禁均未放宽。

## 续接

1. 复用本轮材料入口与242/243原读者，补运行时身份及来源绑定。不要增加第二套账本或调度器。
2. 落实无管理模型调用的真实单执行者基线；当前API路径仍依赖Assign管理来源，不能用合成管理响应冒充真实基线。
3. 补全12任务分支验收接线，冻结实际模型、单槽/总预算后再申请真实实验；72槽仍0次真实，旧有限次授权不得复用。
4. 保留正式签名、安装GUI/升级回滚、独立Windows、供应商/故障矩阵等既有缺口，不以本轮替代整体验收。

本轮0模型调用、0桌面动作、0派发；无Rust构建、打包、安装、凭证使用或信任变更。账户实时额度不可可靠读取，95%收尾规则保留；目标active，releaseAccepted=false。
