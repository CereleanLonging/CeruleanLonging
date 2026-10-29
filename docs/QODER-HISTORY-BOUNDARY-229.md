# 229 新聊天历史边界修复

## 结果与继承

用户选定的Qoder隔离聊天已完成两条初始化，2次实际发送、0重发；两条均正确回复各自标记，成功状态分别显示35s/1次模型迭代、8s/1次模型迭代。原消息、回复、终态、UUID和cwd已核对。旧Qoder GLM接入与WorkBuddy203/206结论保持，没有重跑旧基础验收。

新会话 `0fa16906-4f98-4f59-8b4b-9ea6b556982a`，workspace `9d2371a2-6459-4e9b-91f7-1cafee0fe3f3`，cwd为 `D:\test\CeruleanLonging\reports\qoder-product-228\project`，模型 `byok:80fc8950-525d-4afa-a5ab-11e23fdbae44`；UI为自定义 `Qwen-3.8-Flash, 中`，设置卡再次确认阿里云中国区Token Plan。没有读取Key；不将UI配置或成功短回复等同于计费准确性、模型真实性认证或编程能力验收。

用户已询问能否最小化Qoder，并已告知可以。当前两个请求均已结束，不再要求其保持前台，不再操作该窗口；下一次需要实机时再通知用户。

## 发现与修复

两轮已成功，但原正式TaskPrepare返回 `trial_history_unavailable`。对照原型、现有DesktopTrial.Rounds与当前UIA结构：新聊天有唯一“消息列表”、两组“一轮任务”及完整Agent/回复操作/成功状态，但缺少旧聊天的“消息锚点”。不能把该读取拒绝当作模型任务失败，也不能重发两条消息。

生产变更仅 `crates/core/native/DesktopTrial.cs`：

1. 旧锚点存在时保留旧历史边界与摘要算法。
2. 锚点缺失时，读取实际消息列表的ControlView后代runtime ID，要求非空、无重复、长度受限，并与原观察快照中紧随消息列表的连续节点序列完全一致。
3. 以该真实子树结束位置为边界；不把剩余页面、侧栏、输入框或列表外伪造回复纳入历史。
4. 重复锚点、节点变化/重排/缺失、越界均拒绝。原两轮历史下限、UUID、模型、草稿、忙闲、一次派发保护不变。

临时开始的Rust runner准备改动已撤回至229开始的逐字快照；`project_planning.rs`、228两个产品测试模块及旧WorkBuddy runner哈希均与229开始一致。没有覆盖他人改动。本批目录名称沿用 `qoder-product-runner-229`，实际交付是当前发现的生产历史读取修复，真实Qoder产品runner尚未完成。

## 验证与证据

- `tests/qoder-history-boundary.mjs` 13项边界检查通过：旧锚点、新无锚点、列表外伪造轮次排除、重复/缺失/重排/越界拒绝；证据 `reports/qoder-history-boundary-229-7G5Hkn/result.json`。
- 新生产桥接独立编译成功，原会话只读TaskPrepare通过，2条历史摘要、草稿空、空闲、UUID及模型一致。没有发送任何第三条消息。
- 原桥接失败回执保留于228；新桥接摘要及结果见 `reports/qoder-product-runner-229/native-build.json`、`native-taskprepare.json`。
- 核心桌面定向回归56项通过、23项专项忽略（不计作通过）；fmt与严格核心all-targets Clippy通过。回执退出码0、两路日志关闭及摘要均已核对，最终索引为同目录 `result.json`。

## 下一步

用户问及OMP是否测过，已只读复核196：1个真实原任务、1条建议、独立产物检查与最终Accept、两次重开无重发均通过；13份冻结归档摘要及退出/日志回执一致，当前OMP18.4.4可执行文件SHA仍为 `5644e5d7c7cddd851d79999dbc3b98363817178b33426b355a20fdcad41a3f54`。不重复真实调用、不读取凭证；196主副指挥是固定测试响应，不代证OMP真实三级指挥链。见 `reports/qoder-product-runner-229/omp-existing-evidence-check.json` 与 `docs/OMP-ACCEPT-RECHECK-195.md` 顶部196更新。

229本地回归终态、源码快照和结果已经保存，用户可继续使用其他程序。接下来完成Qoder真实产品prepare/once执行器：复用228受控流程和203/206持久派发/独立检查/重开机制，处理已存在的空隔离项目目录；不要删除该目录重建，不改供应商DB。首尾仍用合成管理响应，用户没有提供新的DeepSeek凭证。

下一次实机准备时，重新发现窗口，核对同一UUID/cwd/model及这两条原历史后，再在批准前冻结最新观察；不要把最小化前的窗口快照直接用于发送。完整编程范围、可执行文件、参考与验证器摘要必须在一次真实sum任务前准备可审阅，不能把本次两条初始化授权用于编程任务。

196/206/218、222/223及224成果继承。冻结0.1.13尚未包含224及229生产修复；本批没有重打包、安装、签名、变更信任。安装后GUI/替换回滚、独立Windows、正式签名、桌面严格监督、其余供应商/可靠性/效率及全要求证据核对仍待完成。目标active，releaseAccepted=false。账户实时额度不可用，不声称95%监控有效。
