# 360：Win10介质已确认；只读检查器序列化故障已处理

最终回执：[primitive-final/result.json](../reports/hyperv-media-360/primitive-final/result.json)，SHA256 `6bad45f544b90848a4d07dbca7c16f444cc4a5de67c6315cc8bfefc08a6c9b2e`。原PID34080已退出，本次只读ISO挂载已卸载，没有启动VM或操作宿主分区。

实际镜像共5项；本轮选用索引3、EditionId=Professional、中文Windows 10专业版、architecture=9（x64），WIM版本记录为10.0.19041.3930。不能仅凭WIM底层版本代替安装后的实际22H2/build19045状态，后续从客体核查。镜像展开大小约16.12GB，32GiB虚拟磁盘能否容纳完整安装/依赖仍由现场结果确定，不自动扩盘。

原VM仍为Off，唯一硬盘为既有system.vhdx，虚拟大小34,359,738,368字节、当前文件约4MiB，逻辑扇区512/物理扇区4096，无检查点。361的EFI100MiB布局所需512逻辑扇区条件已取得实际证据。仅读取键盘WMI方法声明，未发送按键。

## 原失败及修复边界

首次只读脚本完成DISM及ISO卸载后，在最终JSON序列化时异常增长，结果文件保持0字节。先对核实PID21756发送Ctrl+C，未退出；之后通过工具明确批准结束这个自有只读序列化进程，保留6份已完成DISM日志和空结果。停止前私有提交内存约20.40GB；未结束用户应用，也没有安装/模型/VM任务在该进程中。不能把未发布的首次结果视为成功。

第二版限制回执大小、转换DISM复杂类型后，PID3160结束但没有发布最终回执；具体最终异常没有捕获，保留其6份日志及失败状态，不猜测退出码或把它写成通过。

实际Windows PowerShell 5小样证实：Get-Content -Raw返回的文字带PSDrive等扩展元数据，相同文字嵌入JSON产生1690字节（depth2），IO.File.ReadAllText仅57字节。由此修正EI.CFG为纯文本读取，保留显式基础类型转换，增加每镜像独立回执及有界fatal错误记录；在相同PS5环境运行实际投影/发布函数小样，结果1354字节且无附加PSDrive。第三版实读最终成功，不能由此抹除前两版故障。

原始证据集中在[360目录](../reports/hyperv-media-360)：cancel-own-reader、runaway-reader-stop、ps5-json-check、primitive-readback与primitive-final均保留。两个失败检查器不重跑。

## 后续准备

361纯函数应答模板5项假值结构检查通过；未生成真实账户或密码。Bandizip制作/列举/解包一个无秘密小型ISO均exit0，根Autounattend.xml文件名和内容逐字节一致，ISO606,208字节。见[工具小样](../reports/hyperv-iso-tool-check-362/result.json)，摘要 `fb941d387b1c6cfac8518041a470d477b98d30fa7d77f22b6aa21006f550f5af`。

下一步将实际索引3、原VM唯一硬盘及扇区事实绑定到受保护应答介质准备工具，凭证只走本机DPAPI和受限目录，不进入CLI/聊天/普通日志；然后才在原VM内运行普通Windows Setup。不能把纯XML结构、介质工具小样或本次OS版本读取当Windows安装通过。测试总量上限120GB；正式验收未完成。
