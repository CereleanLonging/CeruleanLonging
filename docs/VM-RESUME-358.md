# 最新入口：原 Win10 客体已安装 0.1.25，下一步 GUI 首启

2026-10-26（UTC+8）。原 VM `1925ebd9-f07c-4432-847d-97b6ba1e69c7` 的 Windows 10 19045.3930 已完成安装；0.1.25 候选也已实测单次安装成功，原 NSIS PID3764、Python PID5792、宿主 PID7400 均正常退出。7个安装文件与冻结 manifest 一致，8视图登记核对通过，Direct 会话已关闭。见[384实机根核](../reports/hyperv-install-only-384/live-reviewed-result.json)及[当前交接](HANDOFF-LATEST.md)。

下一步针对现有客体、现有安装准备交互桌面 GUI 首启、正常退出及重开。安装路径为 `C:\Acceptance\CeruleanLonging\reports\isolated-nsis-383-B-candidate\installed app`；标准产品数据目录仍为空。必须实际观察交互会话中的窗口与产品可用性，后台进程存活不代替 GUI 验收。

不重建/启动第二台 VM，不重跑原安装，不重新传输383六文件或生成原计划。WebView2 156.0.4314.42为预先安装，本轮没有测试缺依赖引导。默认安装路径、GUI首启、跨用户、升级/恢复/卸载等不能由本次自定义目录首装推定通过；27项正式门保持原范围。测试总量上限120,000,000,000字节。

下方为原冻结阶段的历史说明，涉及“未创建/未安装”、0.1.24或90GB的旧状态均不再代表当前事实。

---

# 重启后的最小续接入口

> 2026-10-25后续更新：用户已重启，Hyper-V正常运行；359已创建VM并核对配置，仍为Off，Windows尚未安装。用户将测试总量上限提高到120GB，覆盖下文旧90GB表述；后续使用更新的容量检查。不要重新创建同名VM或重跑创建入口，优先读取359回执。

当前Hyper-V启用回执：[host-admin-result.json](../reports/hyperv-acceptance-356/host-admin-result.json)。2026-10-25 18:39已成功启用，Windows要求重启；本次未自动重启，也未创建或启动VM。若续接时系统尚未重启，不重复Enable-WindowsOptionalFeature或重弹旧组件准备脚本。

1. 先读取最新交接及用户回复。核实宿主LastBootUpTime晚于本轮host-admin-start，Hyper-V模块可由Windows PowerShell加载、vmms运行、可用内存至少6GiB；保留Clash Verge及代理。尚未验证的模块参数不能以脚本语法通过代替。
2. 复核[创建脚本](../scripts/create-hyperv-guest-358.ps1)及其[语法核查](../reports/hyperv-acceptance-356/guest-creation-script-check.json)。默认只做准备检查、落报告，不创建；通过后才显式使用`-Create`。只允许选择已存在的虚拟交换机；不指定时网络断开，不创建交换机、不改宿主代理。脚本会重新核ISO、固定资源计划、90GB预算和原次意图，发现已有VM/意图时拒绝重跑，保留部分状态供检查。
3. VM配置为32GiB动态盘、2GiB启动/最大4GiB内存、2vCPU，无检查点、禁自动启动；创建后仍为Off。启动和实际Windows安装前重新核对容量、内存及VM配置，不自动扩大磁盘。冻结预算含ISO和最大磁盘/保存状态余量；当前尚未验证32GiB是否足够实际安装与后续更新。
4. 使用用户现有Win10 x64 ISO。记录实际edition/build、独立machineId、用户SID、系统依赖、八格注册表与标准产品数据目录空白基线。不要复制宿主业务数据/凭证/整个工程；不以新账户替代全新系统基线。
5. 先测0.1.24干净客体首装、首启、正常退出重开，再考虑其他账户的源0.1.21升级恢复。WebView2采用“先记录并准备运行时、再测试产品安装”的分支，不能称缺依赖自动引导已测或产品完全离线。安装器缺运行时会下载bootstrapper；未核查依赖不得直接调用旧静默安装命令。
6. 源宿主指纹已保存：[origin-host.json](../reports/hyperv-acceptance-356/origin-host.json)，SHA256 `7f5b4ce2dd113600addf798e0aac929773f7fdfc6497455dd64f4a542a2bef6c`。向客体复制原字节并核摘要，**不要在客体重新生成来源机指纹**。358首装包装工具的prepare/check材料见其新目录，具体客体计划必须来自客体当前状态，不能用受控fixture充当实机基线。
7. 356旧runbook的首装run参数有路径错误：旧helper要求项目reports的直接子目录并以`isolated-nsis-`开头。应使用`reports/isolated-nsis-358-B-candidate`；不能使用嵌套的`hyperv-install-binding-356/install-B-candidate`。356冻结原文不覆盖，本文件和358新说明纠正。旧helper不是完整客体安全入口，不直接绕过358准备检查运行。
8. 完整升级/恢复仍被356入口阻断；357已否决仅用严格父目录pin绕过文件交接缺口。不得删除阻断条件、修改旧冻结包或将干净首装通过写成升级恢复通过。签名/费用合同/其他供应商门槛继续独立保留。

此页是续接步骤，不是执行回执。当前0安装/VM启动/模型调用/构建；正式验收未完成。账户实时额度不可可靠读取，收到95%可靠账户用量信号立即保存并收尾。
