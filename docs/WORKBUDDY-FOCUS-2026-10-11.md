# WorkBuddy 5.7.7 焦点适配

本次故障定位承接059的真实发送失败，不重放原发送来采集异常。独立诊断只核对原“工作台接入验证”聊天、Hy3、空草稿/空闲和至少两轮历史，再处理焦点；不输入消息、不调用发送按钮。

实机证据 `reports/workbuddy-focus-diagnostic-p9AiUq/result.json`：历史、发送控件、原窗口前台验证通过；输入框启用，但 `IsKeyboardFocusable=false`，`SetFocus()` 抛 `InvalidOperationException`。这复现了输入前的故障条件；059没有阶段日志，不能据此追溯证明其每一个执行步骤。

受保护的点击可以使该输入框获得焦点。点击必须落在新读取的输入框中心；点击前核对原前台进程/窗口、顶层遮挡窗口和 UIA 命中元素；修饰键按下时拒绝。点击后重新读取，原审阅的完整观察仍须一致。首次聚焦可能改变内容/布局摘要，此时拒绝输入，不自行更新原审阅来绕过变化。宿主后续需提供“聚焦后重新准备审阅”的清晰流程。

实机同时暴露两套焦点标识：内容提供器的唯一 Edit 报告 `HasKeyboardFocus=true`，全局 `AutomationElement.FocusedElement` 则返回宿主窗口下的直接 Document。证据 `reports/workbuddy-focus-diagnostic-x4PuUX/result.json`。新增兼容分支仅限WorkBuddy5.7.7：全局对象必须为同进程的Document，运行标识为动态原hwnd对应的 `42.{hwnd}.4.3`；直接父对象必须是同进程、原hwnd的Window；当前内容树仅一个可见Edit拥有焦点，且必须是原输入框。原精确运行标识匹配分支保留。其他窗口、进程、文档、多个获焦Edit均不满足该条件。

生产焦点检查在实机无键盘输入条件下通过，见 `reports/workbuddy-focus-diagnostic-Z1htbY/result.json`。该报告中的旧字段 `inputEvents=0`仅表示未输入文本；诊断曾点击聚焦，不应解读为没有鼠标事件。脚本已改为`keyboardInputEvents=0`、`sendInvokes=0`和`focusMayClick=true`。

受控原始UIA窗口新增不可SetFocus的输入框，实际单击聚焦、Unicode输入和单次发送通过。误命中目标、聚焦后出现用户草稿在键盘输入前拒绝。最终26场景见 `reports/workbuddy-send-native-KkVOCW/result.json`。这些是受控窗口证据，不是实机发送验收。

发送流程stderr仅输出固定阶段枚举：review、observe-before-input、focus-composer、verify-focus、keyboard-attempt、verify-draft、invoke-attempt、read-reply。不输出提示词、草稿、聊天正文或账号信息。记录attempt表示即将调用，不能证明副作用已完成；超时或未知结果仍禁止自动重发。JSON响应协议保持不变。实机验证脚本保留stderr以便后续定位。

用户已再次确认：完整回归通过后，最多重试两条原标记消息，第一条成功才发第二条，失败立即停止。此文件是修复证据记录，最终实机结果以060交接和报告为准。


实机再次尝试结果：reports/workbuddy-send-live-7CgjNy的固定阶段已走到发送按钮Invoke及read-reply，但45秒外层观察超时。只读核对reports/workbuddy-draft-diagnostic-13nJOK：87字符草稿与批准消息完全一致、原4轮历史未变、提示词不在历史、空闲。焦点和文字输入问题已越过，发送按钮Invoke没有产生可核实提交；第二条未尝试。保留草稿，不自动重发。后续需适配有界真实发送点击；现有maxWaitSeconds×2轮数还包含每次UIA读取耗时，应改为真实时限并保留外层回收保护。
