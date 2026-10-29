# 270 便携证据集合与离线验证

2026-10-23T02:55:58.923642+08:00。复用269已冻结索引，不重跑任何业务/模型/独立验收器。新增scripts/portable-evidence.py（仅Python标准库），对明确选定且由独立预期摘要约束的索引封存全部文件。原冷备份的路径与恢复规则保持；便携证据不接管工作台。

## 格式与操作

目录包含manifest.json、README.txt和payload/，原相对文件名保留。清单固定来源索引摘要、每文件大小/SHA256和总大小，executionAllowed/restorableWorkbench/realityAttested/releaseAccepted始终false。验证不使用源工作区，也不执行所附程序、读原路径或联网。归档目录可整体复制；必须从独立回执取得预期清单摘要，不能拿被检清单自己计算的值当可信预期。

创建命令：

```powershell
python -B -X utf8 D:\test\CeruleanLonging\scripts\portable-evidence.py create --root <源工作区绝对目录> --index <冻结索引绝对路径> --index-sha256 <另存的索引摘要> --to <工作区之外的新绝对目录>
```

核查本轮已迁移归档：

```powershell
python -B -I -X utf8 "D:\test\cerulean-portable-270\verifier\portable-evidence.py" verify --archive "D:\test\cerulean-portable-270\relocated" --manifest-sha256 9cce40792cf0196453d929ec079e29d81779eb35d931696e098cad6fd9f0b785
```

工具源码SHA256：04694c4b11e6fe83ea3d688257358420908a2fa945d8038f8bbc6d5d5d20801c。索引含已知凭证路径、链接/重解析点/硬链接、大小超限、重复/大小写别名、路径越界或未匹配内容时拒绝。创建先独占新目录，全部复制/校验后发布清单；失败保留pending，不自动修复、覆盖或重试。Windows读取保留父目录/文件句柄，防止读取期删除/修改。上限10000文件、单文件64MiB、总512MiB、索引4MiB；超限明确拒绝，不静默少导出。

## 本轮证据

- 269完整1334文件，90601251字节（约90.6MB），来源索引d3f060beac20cebb01e8a5afd5b6d024e16c05418dd6461c60ca0365ee71a812。
- 原归档：D:\test\cerulean-portable-270\original；迁移副本：D:\test\cerulean-portable-270\relocated。两者清单摘要9cce40792cf0196453d929ec079e29d81779eb35d931696e098cad6fd9f0b785。
- 迁移验证用独立复制的脚本，在源工作区外CWD、Python -I隔离环境执行，1334项全部通过；未复制Python依赖或导入仓库模块。另有临时夹具先改名源目录并删索引，再验证归档通过。实际平台仍为本机Windows，未宣称独立机器已测。
- 5个测试覆盖移动/源不可用、篡改/缺件/多件/多空目录/pending/清单替换、凭证名/危险路径/重复字段/授权字段/嵌套目的地、失败保留/覆盖拒绝、实际硬链接/Windows目录junction及文件写入锁。
- 实际完整副本错误预期摘要和原目录覆盖请求均按预期exit1；其余3捕获过程exit0，5原进程及输出关闭回执核对。原269全部文件摘要保持。归档内的程序只是字节，不曾执行；0模型/凭证/外发。

本轮仅工程工具及验证，Rust/前端未改，不追加Rust构建。source-ready/verified为最终脚本快照。原发行0.1.15/reader13清单93b8ac9b76772d24ae0af1e43dfa899094d7d03ae3e2a3e78c744b729573e51f保持，开发reader15，未打包/安装/签名/建密钥/改信任。

## 后续范围

- 本轮提供标准库Python离线工程工具，尚未接入产品Rust CLI/桌面入口或安装包；不是跨操作系统/干净机器验收。
- 验证的是所选证据集合的摘要完整性及限定结构，不在新目录重放业务账本、不证明供应商调用真实性或完整实验收益，也不能恢复工作台。需另保留预期清单摘要；不是发布者签名。
- 按明确索引读取并拒绝已知凭证文件名，不递归导出源目录中未选内容；这不是通用文本脱敏，日志/源码可能包含用户内容和原路径，分享前需按实际内容审阅。本轮没有外发。
- 完整首批/CLI/桌面故障、严格原桌面任务监督、真实金额/效率72槽及当前候选实际签名/独立Windows安装仍未整体签收。

目标active、releaseAccepted=false、真实效率0/72。238无需重发或前台。账户额度不能可靠读取，95%账户停止规则保持，不能用context占用替代。
