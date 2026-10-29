# 228 第一条已成功，第二条待续

用户授权续接原草稿后，第一条仅Invoke一次，成功回复 `CL_QWEN_INIT_228_A_764390ef`。原消息、Agent正文和“状态：成功，耗时：35s，模型迭代：1”均按原DesktopTrial.Rounds边界核对。未将回复操作栏文字并入正文；最初直接取Agent组全部后代会包含操作栏，该只读提取错误已纠正，不是模型回复错误，未重发。

新会话UUID为 `0fa16906-4f98-4f59-8b4b-9ea6b556982a`，cwd为 `D:\test\CeruleanLonging\reports\qoder-product-228\project`，workspace ID与用户选择一致，model为 `byok:80fc8950-525d-4afa-a5ab-11e23fdbae44`。原生inspect通过；原先“无新会话”“0真实消息”只适用于之前阶段，现有1条成功初始化消息。没有读取Key，没有编程产物。

第二条输入前，精确焦点检查失败。只读证据发现前台hwnd461294/PID25292属于QQ，Qoder编辑器却仍报告HasKeyboardFocus=true；依赖编辑器单一焦点标志会误输入，所以保护必须保留。第二条0字符/0发送，无自动重试。

新增显式AppActivate后，无输入焦点检查通过，前台窗口和全局焦点均与Qoder匹配。准备文件 `reports/qoder-product-driver-228/initialization/second-message-activated.ps1` 保留原会话、模型、工作区及首条成功凭据保护，在输入前检查实际前台与精确编辑器runtime ID。输入和回读均有有界等待；使用v2-second独占意图，不覆盖原失败记录。尚未执行修复后的第二条。

剩余范围仅原计划第2条标记 `CL_QWEN_INIT_228_B_f18c257b`，不读写文件、不调用工具、不运行命令；最多1次发送，失败/未知停、不重发。需要用户允许从此次停止继续，并在短暂输入期间保持Qoder前台，避免QQ或其他窗口抢占。第一条已经通过，禁止重发。

成功后用原正式TaskPrepare冻结两轮历史基线，再准备产品编程runner与具体sum任务；初始化不等于编程闭环或正式验收。

证据在 `reports/qoder-product-driver-228/initialization/`：`first-proof.json`、`first-session.json`、`first-native-inspect.json`、`second-stopped-before-input.json`、`second-focus-only-result.json`、`current-result.json`。目标active，releaseAccepted=false；账户额度仍无可靠实时读数。
