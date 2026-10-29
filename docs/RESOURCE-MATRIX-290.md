# 290：声明资源矩阵、未知任务与显式修订的离线验收增量

2026-10-23 15:52（UTC+8）。本轮只新增一项真实 Git／原项目调度／独立验证器夹具；主、副指挥及执行者响应均为受控回执，模型调用和桌面动作各 0。286 的 A6API `gpt-6.1-sol` 真实编程闭环已通过，本轮不重发原任务；279 的失败和 284 的 unknown 仍保留。

## 结果

`host::integration_tests::parallel::declared_resource_matrix_survives_unknown_then_requires_new_approval` 在新隔离的 `reports/resource-matrix-290/case-attempt5` 通过（1/1，67.97 秒）。项目方案将 a、b 绑定同一组显式资源名，c 不占用该组资源。原批准运行时 a 与 c 并行，b 等待、无启动和 DispatchIntent；c 独立验收通过后，a 的受控 HTTP 503 留为 Unknown，b 和最终集成均未派发。第一次冷重开保留原账本和资源冲突，恢复原尝试无封口结果而被拒，旧批准不能重新启动；第二次冷重开仍保留原历史。显式退役 unknown 后资源冲突解除，但旧版 b 仍不被自动派发。用户修订请求、配置新版、取得第 2 版方案并批准后，新运行完成 a、c、b、integration 四任务独立验收与主指挥 Accept；最终冷重开历史一致。

持久事件计数为 a 2、b 1、c 2、integration 1；原 unknown 尝试重发 0。这里的 `tcp-resource-matrix-290` 与 `db-resource-matrix-290` 只是**单个工作台核心内声明的资源名**，测试不证明跨进程端口锁或真实数据库排他。原案、新案批准摘要不同，最终集成产物为 `AB`。受控夹具中的 Key 为合成测试值，不使用用户中转凭证。

## 夹具修正与证据边界

前四次失败均保留：attempt1 为 Windows 路径规范比较；attempt2 缺少显式 `CERULEAN_TEST_GIT`；attempt3 直接索引 API 协调器持有期间暂离宿主 map 的项目账本；attempt4 在新方案异步完成前读取了旧第 1 版。修正只涉及 `crates/core/src/integration_tests/parallel.rs` 的测试夹具：执行期间从原子发布的原事件文件计数，最终账本等待 map 恢复；新方案明确等待第 2 版落盘。没有修改产品资源租约或恢复逻辑。目标单项测试与该文件 `rustfmt --check` 通过，未因此宣称全量回归通过。

冻结索引为 `reports/resource-matrix-290/latest.json`；有效源码副本在 `reports/resource-matrix-290/source-final`，原测试前源码副本和摘要在 `reports/resource-matrix-290/before` 与 `before-source-hash.json`。旧失败日志及现场均未改写。

本轮只补 P1 资源等待及 Unknown 后显式修订的离线组合覆盖。完整正式验收仍为 `releaseAccepted=false`：首批其他供应商/CLI、严格原桌面任务监督、真实金额、72 个真实效率槽、独立 Windows 安装升级回退及正式签名等缺口沿用 288/289 表。账户额度没有可靠实时读数，95% 停止规则继续有效。
