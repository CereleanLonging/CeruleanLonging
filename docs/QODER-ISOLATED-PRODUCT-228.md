# 228 Qoder 隔离工作区与产品执行器准备

2026-10-20。继承227的迁盘及Token Plan设置观察、原Qoder GLM接入、WorkBuddy203/206产品流程，不重新执行旧基础验收。

## 本次实际完成

用户选择“新建隔离项目验证聊天”，明确本步尚不发送模型任务。通过Qoder正常UI创建工作区“工作台 Qwen 验收 228”，唯一可读写目录为 `D:\test\CeruleanLonging\reports\qoder-product-228\project`，工作区索引关闭。只读数据库核对 workspace ID 为 `9d2371a2-6459-4e9b-91f7-1cafee0fe3f3`，root_paths_json与上述路径完全一致，目录为空；没有直接写Qoder数据库。

原“工作台接入验证”会话 `76edb3bc-078f-4ce4-bc9b-b6525fde9c51` 保留，cwd仍为 `D:\qqagent`。新工作区页面尚无持久chat_sessions记录，因此准确状态是“工作区已建、待初始化聊天”，不是“新聊天绑定通过”。

仅参数化测试执行器 `workbuddy_product_live.rs` 的应用、模型、执行者及管理夹具，新增 `qoder_product_probe.rs`。旧WorkBuddy入口、owned runner、203只读恢复函数保持；恢复函数与228前快照逐字比较一致。Qoder采用原RuntimeHost、一次派发、15项Node产物检查及两次重开机制；管理响应与桌面传输均为本地合成，未调用供应商。

本批不修改生产调度或原生桥接。新增模块尚无真实发送入口，不能把本地演练当作完整实机执行器已冻结。

## 验证结果

- 2项聊天/模型/进程/历史保护测试通过，包括Qoder缺失或变化的UUID、空历史、忙碌及草稿拒绝。
- Qoder受控产品流程通过：1次合成发送、独立15项文件检查、Accepted、两次重开无重发，合成无效凭证已移除。
- 原WorkBuddy受控产品流程通过，覆盖共用函数的变化。
- `cargo fmt --all -- --check`、核心 `clippy --all-targets -- -D warnings` 通过；实际退出及两路日志关闭回执、摘要均已核对。
- 首次编译引用未引入的uuid crate失败，改用既有UUID格式规则；首次演练使用占位版本被产品正确拒绝，夹具修正为0.4.3.0后通过。原失败记录保留，不降低产品版本保护。

证据：`reports/qoder-product-driver-228/result.json`、`source-manifest.json`、各receipt；Qoder成功演练 `reports/qoder-product-rehearsal-228-gD0S6b`，WorkBuddy成功演练 `reports/workbuddy-product-rehearsal-y7U4lM`。

## 新页面与当前进程差异

原生taskprepare在新页面返回 `draft_or_composer_unavailable`，没有发送。反查既有桥接与原型STEP-02：新页面尚无聊天UUID；当前Qoder TaskPrepare明确要求至少两轮已完成/取消历史作为后缀关联依据。不要伪造历史、修改供应商数据库或放宽原会话保护。需要为用户新选的工作区初始化历史，与重复旧接入验收是不同事项。

续接中Qoder已换为PID11116，由 `E:\千问\Qoder\.qoder-versions\0.4.3\Qoder.exe` 启动；该E盘目录仍是指向 `D:\Qoder\app` 的联接，桌面数据联接仍指向D盘。旧PID27984及窗口证据不可用于发送。新进程最初只暴露外壳，后续UIA文档恢复，无需修改桥接或重启。

当前新页面模型仍显示 `Qwen-3.8-Flash, 中`。自定义菜单项已变为 `Qwen-3.8-Flash bailian`，本轮已选中此项。227记录的菜单供应商全名与当前别名不同；仅名称不能代证当前端点、套餐或凭证有效性，发送前须在设置UI重新核对Token Plan类型，不读取Key。当前没有持久新session model可交叉验证。

## 下一步的初始化范围

待用户确认的精确计划为 `reports/qoder-product-driver-228/initialization-plan.json`。最多两条消息，顺序发送；每条只要求回复不同标记，不读写文件、不调用工具、不运行命令、不联网。模型请求本身通过Qoder联网，前述“不联网”是对执行者工具行为的约束。

1. 重新发现实际窗口，检查唯一新工作区、实际目录、Token Plan自定义模型、空草稿、空闲状态。条件不符停止。
2. 发第1条初始化提示前独占保存意图；只操作本次新工作区页面。最多等待90秒，未知/失败立即停，不重发。
3. 只读确认新UUID、cwd、模型及第1条原消息和回复。成功后同一UUID内发第2条，按相同规则保存和回读。
4. 两轮均成功后复用正式TaskPrepare冻结基线，保存新session/model/cwd/reference。初始化不代表产品编程验收。
5. 完成Qoder实机prepare/once runner、精确计划摘要及工作台独立验证范围后，再安排最多1条sum.mjs任务；本次两条初始化范围不包含编程任务。

目前没有发送初始化消息，也没有编程调用；不复用此前一次性Key或已消费的有限发送授权。

## 正式验收边界

目标active，releaseAccepted=false。196/206/218与222/223证据继承，224生产修复尚未进入冻结0.1.13。安装GUI/实际替换回滚、独立Windows、正式签名、桌面严格监督、其余首批供应商与可靠性/效率仍按既有门槛推进。227清理被拒绝的两份旧备份继续保留，不重试删除，不把其体积记作释放。

当前无可靠账户额度遥测，不能宣称95%自动监控有效；若收到新可靠读数达到95%，按既定规则安全收尾。
