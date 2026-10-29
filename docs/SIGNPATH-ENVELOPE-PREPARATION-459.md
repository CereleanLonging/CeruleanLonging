# 证书表结构与载荷审计

续接458。`scripts/review_nsis_payload.py` 新增显式选项 `--allow-certificate-envelope`，默认行为仍拒绝追加数据。新模块按[Microsoft PE格式说明](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format#the-attribute-certificate-table-image-only)检查证书表的文件偏移、边界、八字节对齐、节区不重叠及零填充；当前仅支持文件末尾的一条 revision 2 / PKCS_SIGNED_DATA 记录，其他布局拒绝。

在核对结构后，原NSIS CRC、压缩流边界及七件命名载荷摘要检查继续执行。整体安装器摘要仍包含证书表。证书内容保持不透明，结果明确 `signatureVerified=false`、`publisherVerified=false`、`releaseAccepted=false`，不把结构正确解释为Authenticode有效或发布者可信。

验证：原22项载荷测试、新11项结构边界测试通过；451冻结包和458打包验证产物的默认/显式读取结果一致。在内存中给两份真实包添加明确标注“非签名”的合成证书记录后，七件载荷摘要仍相同，默认模式仍拒绝追加记录。原文件不改写、无程序启动。第一次只读夹具因硬编码记录长度断言失败已保留；计算实际填充长度后的独立attempt-2通过。见 `reports/signpath-envelope-459/result.json`。

尚未获得真实签名样本。真实SignPath产物到位后仍必须验证签名链、PE映像摘要、发布者、时间戳、吊销及安装后文件；本次只关闭本地结构读取准备缺口。458说明中“外层读取器仅支持未签布局”的状态以此增量为准，其余签名/公开授权边界保持。
