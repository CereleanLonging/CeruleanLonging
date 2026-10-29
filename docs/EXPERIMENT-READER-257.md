# 257 实验目录读者保护与冻结旧核心核查

时间：2026-10-21T07:17:00.482354+08:00。本轮兼容差异验收通过；正式验收false，目标active。

## 当前行为

新增reader14，保护Host的baseline-experiments、team-material-bindings、team-material-import（含pending）、team-observations、team-timelines。常规Store打开前检查未保护的旧实验记录；启用标记后，冻结0.1.15/reader13在创建执行Host前拒绝，不会忽略新增实验约束后继续写。

基线产品动作、团队产品动作及团队材料/规划前准备/观察/计时追加等写入口要求保护。已有材料和计时原记录不重写，原主副指挥/执行器/验收/时钟保持；此标记不等于原任务已成功或可恢复。

复用现有冷备份的身份、原位置、清单/字节验证和全部数据文件/嵌套writer锁。新增with_original_cold仅用于明确的离线元数据提升：不创建或释放升级hold，不绕过已有hold、维护预约或恢复事务。必须停止核心，review与promote均重核原数据与冷备份一致；不完整发布仍用原.format-migration pending规则拒绝启动。保留旧with_original_held行为。

这样可处理新核心已拒绝打开的未保护实验数据，不必先启动它来取得升级hold。CLI：

```text
cerulean-runtime.exe --workbench --data-dir <数据目录> backup create --to <新冷备份目录>
cerulean-runtime.exe --workbench --data-dir <同一数据目录> data-format review-experiments --from <冷备份目录> --manifest-digest <备份摘要>
cerulean-runtime.exe --workbench --data-dir <同一数据目录> data-format promote-experiments --from <同一冷备份> --manifest-digest <同一摘要> --review-digest <审阅摘要>
```

不要给冻结reports原目录补写标记。新实验使用新隔离数据目录；本轮未迁移用户数据。生产UI创建实验工作台时还需接此流程，当前明确依赖CLI，未声称已经完成普通用户的一键入口。

升级控制器现在识别数值reader14、实际标记和格式回复；受保护数据拒绝源或目标读者低于14、字符串/未来代数、不一致回复及pending。维护预约同步支持14并继续拒绝未来15。源码主程序build-info报告14，当前开发版本号仍0.1.15；它是reports内开发二进制，不是原冻结发行包。原0.1.15包仍reader13，未被替换。

## 证据

- 5项Rust保护检查通过：1/3/13代原记录与冷备份不变、明确审阅、活写者/错误或变化备份拒绝、已有hold拒绝、缺失/篡改/未来/pending保护拒绝、未保护产品导入0新写入。
- 10项维护预约差异回归通过，覆盖既有5至13及新增14；未来和非数值声明仍拒绝。
- 1项新夹具导入通过，基线与团队只导入配置，没有规划/批准/派发；旧255/256材料区域逐文件不变。
- 26项PowerShell升级控制器受控检查通过；Core格式响应为模拟，未替换程序或安装。原冻结控制器对14声明的拒绝也保留为对照。
- 实际冻结0.1.15/reader13二进制分别对新保护基线、团队根执行--serve，均退出并报告Unsupported reader guard filename，数据逐字节不变；新reader14真实服务可读取它们，正常停止，不规划/派发。
- 旧二进制仍可启动读取普通旧计划控制组，正常停止且原数据不变。
- 新离线CLI对残缺导入根review、错误摘要拒绝、正确promote、重复拒绝。仅新增reader14标记，原pending和备份不变；新服务仍显示诊断只读，拒绝项目新增，不冒充修复或恢复成功。
- 严格core all-targets Clippy、fmt、核心/CLI测试程序/主程序构建、Tauri cargo check通过。CLI测试程序只编译，未计入通过测试数；新增CLI由实际命令验证。未重跑无改动的前端检查。
- 28个捕获进程均退出且日志关闭，4个成功启动服务以原进程句柄确认正常退出；两次旧核心拒绝也有终止回执。当前无本批遗留进程，未强杀。

0真实模型/桌面发送/用户凭证读取；隔离树无DPAPI文件。没有发布、安装、新建密钥或信任修改。238继承已通过，不重发、不要求Qoder前台。

## 脚本失误保留

产品检查未失败。首轮汇总使用“0 passed”子串误匹配“10 passed”，在维护回归已经通过之后停下；原5+10结果直接复核，未重跑，只续导出未执行夹具。原checks.py和initial-check-summary.json保留。

首次native脚本将JSON路径字符串直接传给Path helper，完成两次build-info后、任何服务/提升前停止。修复为显式Path，复用原版本结果，继续未执行动作；native.py、native-initial-error.json与native-remaining.py均保留。没有因脚本问题重派模型或重做原任务。

source-verified为最终源码，与source-ready已构建文件摘要一致；后加PowerShell测试一并保存。构建/检查脚本已完成，不整段重跑。

## 正式验收与下一步

- 发行版独立实验工作台创建/选择，并在使用实验前完成保护；目前依赖显式离线CLI。
- 已有基线登记冻结同Host新增/重配普通项目，后续应隔离实验与日常工作台。
- 完整进程崩溃、I/O失败、部分导入诊断/处置、便携证据归档尚未全覆盖。
- 真实配置与总预算、72真实效率槽仍0/72；旧Key和有限授权不复用。
- 新候选包、安装/独立Windows/完整Tauri点击、证书/签名/版本/清单一致性按实际候选验收。

优先补发行版独立实验工作台创建/选择，复用本轮离线保护、256后台基线与255团队入口，不再写另一套调度。不要把“受保护目录可以启动”当作新安装Tauri完整流程或真实效率证明。

原0.1.15清单摘要仍93b8ac9b76772d24ae0af1e43dfa899094d7d03ae3e2a3e78c744b729573e51f。签名、证书及其他模型成果保留；未用旧签名结论替代新候选验收。真实效率0/72，releaseAccepted=false，目标active。账户额度无法可靠读取；可靠95%账户信号到达立即安全收尾，不使用上下文占用替代。

权威入口：reports/experiment-reader-257/latest.json、result.json、requirements.json、source-verified.json。
