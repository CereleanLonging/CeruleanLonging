# 359：Win10虚拟机已创建，Windows尚未安装

用户已完成宿主重启，实测启动时间为2026-10-25 18:50:26（UTC+8）；vmms正在运行，Hyper-V模块已可用。Clash Verge、服务及mihomo进程保留。

重启后核对本机Hyper-V参数，发现Connect/Disconnect-VMNetworkAdapter不接受`-VM`，已在创建前修正为`-VMName`。修正前脚本保留；管理员执行前再次核对全部所用参数。首次普通权限读取VM列表被拒绝，没有降低ACL或修改授权组，随后走正常管理员提升。

管理员PID7012已终止并留下success=true回执：[admin-result.json](../reports/hyperv-guest-359/admin-result.json)。原创建意图和结果均保留，不再运行创建入口。

| 项目 | 本轮实际结果 |
|---|---|
| VM | CeruleanLonging-Win10-358 |
| ID | 1925ebd9-f07c-4432-847d-97b6ba1e69c7 |
| 状态 | Off，未启动 |
| 虚拟磁盘 | reports/hyperv-acceptance-356/vm/system.vhdx，32GiB动态盘 |
| 内存/处理器 | 启动及最低2GiB、最大4GiB，动态内存；2vCPU |
| 自动行为 | 启动Nothing、停止ShutDown、自动检查点关闭、CheckpointType Disabled |
| 启动介质 | 用户现有Win10 ISO，原摘要已核对 |
| 固件 | SecureBoot On，MicrosoftWindows模板 |
| 网络 | 连接既有Default Switch，未创建交换机或改动宿主代理 |

原PowerShell JSON中SecureBoot=0、CheckpointType=2等是枚举数值，不能解释为关闭/开启。已对本机模块枚举核对：[hyperv-enum-values.json](../reports/hyperv-guest-359/hyperv-enum-values.json)。回执success只表示创建及配置核对通过，不表示Windows安装、产品安装或正式验收通过。

## 最新容量授权

用户将测试总量上限提高到 **120,000,000,000字节**。已更新AGENTS与容量工具，保留旧90GB工具副本/回执。新容量前检实测：当前测试文件（含外部ISO和新VHDX）43,029,028,263字节；再保守预留42,000,000,000字节仍通过，实际D盘空闲234,922,246,144字节，保留10GB磁盘安全余量。见[120GB回执](../reports/hyperv-guest-359/storage-120gb.json)。动态盘应按将来增长预算管理，不能只按当前小文件判断。

提高上限没有触发重建/扩容/额外清理。下一步在原VM上准备普通Windows Setup，不碰宿主磁盘分区、不复制整个工程或凭证。安装前固定实际ISO中的Windows10版本及系统安装方案，记录客体独立身份和空白基线。

358首装工具仅prepare/check，9项受控检查、后续来源机绑定差异以及根的实际宿主拒绝检查通过；真实安装接线尚未执行。357已经否决严格父目录pin作为writer.lock交接替代，完整升级/恢复仍阻断。保留全部旧接入成果与失败。正式验收未完成，账户实时额度无可靠读数。
