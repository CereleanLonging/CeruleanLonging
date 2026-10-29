# 228 初始化首次停止与修复后范围

用户已批准最多两条无工具/无文件初始化消息；本次首次尝试在输入前停止，没有发消息、没有输入任何字符。原 `first-compose-intent.json` 保留，不删除它来重新运行。

发送前通过Qoder设置中Qwen-3.8-Flash模型卡重新确认 `Alibaba Cloud Model Studio - China · Token plan`，说明当前菜单的bailian别名仍对应该配置类型；没有读取Key或尝试连接。返回新工作区后草稿为空。

原脚本激活窗口并SetFocus后固定等150ms，前台/编辑器焦点条件尚未满足，按约定退出。之后只读核查编辑器与全局焦点runtime ID一致、草稿长度0，数据库中新工作区session数仍0。不能因此自动重复首次尝试。

修复版 `reports/qoder-product-driver-228/initialization/first-message-fixed.ps1` 改为最多2秒等待，必须同时满足目标前台、编辑器焦点和全局焦点精确runtime ID；输入前再核对一次。无输入焦点专项通过，PowerShell语法解析通过。修复版尚未执行输入/发送；它使用v2独立意图名称保留原失败记录。

拟继续的范围仍为原 `initialization-plan.json` 中两条精确提示、同一个隔离工作区与Token Plan自定义Qwen；没有新增编程或文件操作。每条只回复标记，最多两条，首条成功绑定真实UUID并核对原消息/回复后才允许第二条。任何失败/未知停止，不重发。真实初始化不是产品编程验收，后续sum任务仍需具体冻结计划。

结果见 `reports/qoder-product-driver-228/initialization/result.json`，路由确认、首次意图、失败回查、无输入焦点结果均保留。此前约定失败即停，须等用户明确允许修复后继续；此次授权没有产生模型调用，不能写成额度已实际消费。目标active，正式验收未完成。
