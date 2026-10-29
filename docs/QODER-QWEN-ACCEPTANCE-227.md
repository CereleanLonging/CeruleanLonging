# 227 Qoder数据补迁与Token Plan实际配置核对

## 对226范围的纠正

226迁移的五处目录真实通过，但遗漏了实际Electron用户数据 `C:\Users\姚\AppData\Roaming\com.qoder.app.stable` 和空的 `C:\Users\姚\AppData\Local\qoder-updater`。因此226交接所说的“配置和历史均已在D盘”范围过大。旧报告保留，227补充纠正，不能直接继承其完整迁盘结论。

本轮正常退出后，将遗漏目录分别复制到 `D:\Qoder\desktop-data` 与 `D:\Qoder\updater-cache`。11,287文件、770,949,461字节逐个摘要核对通过；数据库关闭后没有WAL，源与目标只读 `PRAGMA quick_check` 均为ok。旧路径改为联接，7处原路径现都解析到预定D盘目录。Qoder0.4.3.0重新运行，实际子进程 `--user-data-dir` 指向原C盘逻辑路径，该路径现解析到D盘。

删除本批两份旧备份的命令被自动审批检查拒绝，返回 `blocked by policy`，没有详细理由；未尝试绕过或更换工具删除。原备份仍保留约735MiB，因此这部分C盘空间尚未释放：

- `C:\Users\姚\AppData\Roaming\com.qoder.app.stable.migration-227-backup`
- `C:\Users\姚\AppData\Local\qoder-updater.migration-227-backup`

源备份重新校验与原冻结内容相同，新目录已运行；旧备份保留不阻断接入工作。226已清理的旧副本和释放空间结论保持。本轮没有再搬226目录，没有升级Qoder。

## 实际Qwen配置与原验证聊天

1. 桌面数据库 `byok_model_profiles` 为0条，只能说明该表为空。Qoder实际模型设置页展示多个自定义模型，不能用单表空值判定用户没有配置。
2. 设置页可见 `Qwen-3.8-Flash` 等模型与 `Alibaba Cloud Model Studio - China · Token plan` 标签。未打开/输出Key字段，没有读取凭证表或解密凭证。
3. 原“工作台接入验证”聊天原本仍选 `GLM-5.3-Flash, 最大`。用户其他聊天显示Qwen不能代证此验证聊天已选择Qwen。
4. 在同一个原验证聊天的模型菜单中，明确切到“自定义”标签，选择 `Qwen-3.8-Flash Alibaba Cloud Model Studio - China`，未输入消息或发送任务。
5. 复用原生桥接 `taskprepare` 成功：sessionId `76edb3bc-078f-4ce4-bc9b-b6525fde9c51`、modelLabel `Qwen-3.8-Flash, 中`、草稿空、非忙碌、4个基线摘要。返回成功证明当前会话可读及原始历史可作前置锚点；不能证明Key有效、套餐实际计费、模型编程能力或调度链完成。

迁盘恢复时此前出现的 `document_not_unique` 不应直接归因为产品损坏；当时存在宣传弹窗，回到正确验证聊天后旧观察机制无需修改即可通过。本轮没有重新执行原GLM20/20、Codex或WorkBuddy已成功基础验证。

## 下一项具体实现顺序

1. 复用 `workbuddy_product_live.rs` 的RuntimeHost流程和原15项独立sum验证器，为Qoder参数化应用、模型、原会话锚点及固定管理测试响应；保留203/206恢复入口与旧证据不变，不另造权威任务账本。
2. 检查原Qoder验证聊天实际工作区；新任务的隔离目录必须与批准范围及应用配置一致。不能仅把 `workspace_confirmed` 写成true，也不能让模型在用户其他业务目录产生文件。
3. 在新隔离目录准备唯一 `sum.mjs` 任务：有限number相加，否则TypeError；不运行命令、不联网、不改其他文件。工作台独立执行固定摘要验证器，成功后检验原任务Accept与重开无重发。
4. 先完成仅本地受控演练及真实只读准备，冻结新计划与摘要，再明确一次真实发送范围。任何失败/未知保存原尝试供回查，不自动重发。
5. 用户没有DeepSeek凭证，本轮可做Qwen执行者真实编程验收，但管理响应若为测试固定值，必须明确标为合成；不能宣称真实三级链完成。Token Plan官方交互式使用范围继续适用，不把桌面模型配置变成任意后台API。

官方依据：[阿里云Qoder接入说明](https://help.aliyun.com/zh/model-studio/qoder-agent)列出供应商、套餐类型和模型的独立配置；[Token Plan个人版说明](https://help.aliyun.com/zh/model-studio/token-plan-personal-overview)限定交互式编程/智能体工具使用。模型配置与实际推理/计费验证是不同证据。

## 结果

`reports/qoder-desktop-profile-227/result.json`、`qwen-task-preflight.json`、`model-settings-observation.json`、`custom-model-labels.json`、源副本与数据库校验记录完整保留。临时全窗口截图已删除，未保留无关聊天正文。

本轮0新模型任务、0用户Key输出、0工作台产品构建。224源码修复与冻结0.1.13差异保持；正式验收仍未完成，目标active。账户额度实时遥测仍不可用，不宣称95%自动监控有效。
