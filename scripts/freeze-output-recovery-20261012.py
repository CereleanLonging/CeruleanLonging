from pathlib import Path
import hashlib
import json
import re
from datetime import datetime

ROOT = Path(r"\\?\D:\test\CeruleanLonging")
INDEX = "reports/p6-output-recovery-20261012.json"
ARCHIVE = "reports/p6-output-recovery-sources-20261012"
VOLUME = "handoffs/2026-10-12/handoff_2026-10-12_003_222417+0800.md"


def digest(path):
    return hashlib.file_digest(path.open("rb"), "sha256").hexdigest()


def read(path):
    return json.loads((ROOT / path).read_text(encoding="utf-8-sig"))


def log_text(path):
    raw = (ROOT / path).read_bytes()
    return raw.decode("utf-16" if raw.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8-sig")


def log_result(path):
    return json.loads(next(line for line in reversed(log_text(path).splitlines()) if line.startswith("{")))


assert not (ROOT / INDEX).exists(), "Preserve frozen report"
assert not (ROOT / ARCHIVE).exists(), "Preserve frozen sources"
prior_path = "reports/p6-backup-interrupt-pending-20261012.json"
prior = read(prior_path)
for record in prior["records"]:
    assert digest(ROOT / record["snapshot"]) == record["sha256"], record

original = "reports/backup-interrupt-window-native-9340188633364a59bf035f2df6d250e7"
reproduced = "reports/backup-interrupt-window-native-90b8cec5a4574ada8c3c53ceffc69a30"
encoding = "reports/backup-interrupt-window-native-627d1a0102964f8fb50dc5fd8e0188db"
controller = "reports/backup-interrupt-window-native-a2b50df5fb84492fb5b84ef9470feb04"
helper = "reports/backup-interrupt-window-native-dda81e0b91d24a66bd80300a1df7acc3"
recovery = "reports/recovery-window-native-3fd2cbd6c2d8424e8ea79d606baf6dba"
accepted = {
    "originalDisposition": read(original + "/resumed2-result.json"),
    "reproducedDisposition": read(reproduced + "/resumed-disposition-result.json"),
    "encodingFixtureCleanup": read(encoding + "/cleanup-result.json"),
    "controllerInterruption": read(controller + "/result.json"),
    "helperInterruption": read(helper + "/result.json"),
    "candidateUpgradeAndRecovery": read(recovery + "/result.json"),
    "outputCapture": log_result("reports/p6-maintenance-output-capture4-20261012.log"),
    "candidateReview": log_result("reports/p6-output-fix-candidate-018-20261012.log"),
}
for name, value in accepted.items():
    assert value["status"] == "passed", name
cleanup = read("reports/p6-output-recovery-018-cleanup-20261012.json")
assert not cleanup["fixtureProcessMatches"] and not cleanup["productRegistered"] and not cleanup["uninstallRegistered"]
manifest_path = "reports/p6-windows-package-018-20261012/manifest.json"
manifest = read(manifest_path)
for artifact in manifest["artifacts"]:
    assert digest(ROOT / "reports/p6-windows-package-018-20261012" / artifact["path"]) == artifact["sha256"]
assert manifest["version"] == "0.1.8" and not manifest["releaseAccepted"]
old_manifest = "reports/p6-windows-package-017-20261012/manifest.json"
assert digest(ROOT / old_manifest) == "811bbea49bad673e43b5bb8a6c6d068483ae7a14357f38514fe2db8b26cfffca"
counts = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored", log_text("reports/p6-output-fix-backup-tests-20261012.log"))
assert sum(int(c[0]) for c in counts) == 26 and all(int(c[1]) == 0 for c in counts)
staging = log_text("reports/p6-output-fix-staging-tests-20261012.log")
assert "2 passed; 0 failed" in staging
at = datetime.now().astimezone().isoformat()
summary = f"""{at}

116补齐备份中控制器/助手中断的独立证据并修复维护窗口，0.1.8内部候选已冻结；完整P3/P4/P5/P6仍未通过，目标active。

先处置115原现场：正常宿主CIM确认旧窗口/控制器/相关助手当前均不存在，原数据与程序清单保持，已发布备份可验证。只续接同一事务的AbandonPreparation和重开已归档核查，再解除原hold、读取原项目并卸载隔离安装，没有重做Prepare。115历史助手生命周期仍无法补证，不把此次当前状态反写成原场景通过。第一次续接只读阶段因PowerShell 5默认解码UTF-8 JSON失败，保留原失败，显式编码后完成。

独立Controller补测复现真实产品缺口：窗口PID5992、控制器25680、助手10076，持有原句柄证明控制器退出后助手仍活着，最终助手退出101。原窗口只读控件证据显示ReadAllText遭stdout共享占用，结果框仍是上次审阅；产品已产生父进程finished回执却未收完日志。已保存原窗口、回执、日志和watcher证据，核验原数据后处置同一事务并清理，没有重发该Prepare。

产品修复：维护窗口由C#直接持有Process及stdout/stderr捕获任务；完成条件为原进程退出、两路EOF、flush及写入器关闭，再生成回执、读取结果和重新启用控件。窗口不阻塞UI，不自动重发；核心/更新器的持久占用和独立写锁保持。新捕获先暴露本地代码页解码UTF-8核心JSON的问题，该次仅coreReview、没有Prepare，已清理。随后明确UTF-8解码，更新器/包审阅脚本明确UTF-8输出。受控继承句柄夹具覆盖父进程先退出、子进程延迟双路输出、各20,000行并发输出、中文/Unicode及启动失败释放日志句柄；首个夹具未实际继承输出及UTF-8脚本未带BOM的失败均保留，不算产品成功证据。

修复脚本配合冻结0.1.7核心的两例真实窗口通过：Controller为窗口/控制器/助手原句柄链，控制器21968被终止、助手18376存活至退出101，备份发布且独立verify成功，不能把101称为助手正常成功；Helper为助手1388被终止、控制器7880退出1，留下60,284,928/268,435,456字节部分payload，无正式备份。两例均原程序/数据不变，实际AbandonPreparation只归档维护占用、保留原hold与全部残留；重开归档核查后明确解除hold，原项目一致，安装已清理。两份报告的frozenToolsVersion表示基线，实际替换脚本以各session.artifacts摘要为准，不称未修改0.1.7已通过。

版本统一0.1.8，备份格式仍为schema1、显式允许0.1.0..0.1.8写入者。26项备份/恢复相关测试通过、1项明确忽略；桌面实际嵌入工具暂存2项通过；fmt、TypeScript/Vite和release/NSIS构建通过。Node沙箱spawn rustc为EPERM，正常宿主构建通过；并非Rust未安装。未因未变化能力重复整套接入，复用112全workspace与clippy，原ts-rs警告保留。0.1.8候选14项审阅检查通过，manifest SHA256 {digest(ROOT / manifest_path)}；NotSigned/releaseAccepted=false，0.1.7冻结摘要保持。release核心、sidecar与包一致；旧debug运行组合未重新同步，不应按0.1.8完整开发桌面组合使用。

新0.1.8实际窗口补验：隔离0.1.7→0.1.8更新及回滚，第二个明确事务升级后注入维护占用改名共享冲突，终态被保留，安装核心启动被拒；重开读取同一事务、FinishMaintenance，不重复Commit。随后0.1.8创建备份，产品自行保留原目录并恢复，重开Apply、Finish/Inspect及未开始恢复的Abandon/Inspect、最后解除hold均通过；原项目不变、清理完成。仍是明确绑定隔离目录的暂存会话，不能代证安装版React→维护窗口完整入口或干净机器。实际故障与正常流详见各原报告，不合并成全部存储故障已通过。

最终CIM无本批夹具进程，产品/卸载登记均不存在。用户暂无独立Windows环境和签名证书，继续其他验收，不重复询问；未调用模型、用户桌面或关机，无提交推送。剩余安装版完整入口、干净机器/跨用户机器、正式签名/服务、程序暂存其他中断点、真实磁盘满/同步失败/断电/共享故障负载，以及原首批API/CLI/Agent/两桌面、真实OpenAI管理/编程/效率、金额预算/按执行者监督及075/085门槛保留。WorkBuddy064不重发。下一步从剩余缺口择项推进，先查本批证据；不要重跑已验收基础接入。
"""
latest = ROOT / "docs/HANDOFF-LATEST.md"
old = latest.read_text(encoding="utf-8-sig")
latest.write_text("# 最新交接\n\n当前：2026-10-12 卷003第005项（116），progress，维护日志收尾修复及0.1.8内部候选/实际升级恢复通过，完整P6未通过。\n\n" + summary + "\n[116验收索引](../reports/p6-output-recovery-20261012.json)。\n\n下方115为冻结历史。\n\n" + old, encoding="utf-8")
with (ROOT / VOLUME).open("a", encoding="utf-8") as out:
    out.write("\n\n## 第005项 / 116 维护日志收尾与0.1.8恢复验收\n\n" + summary + "\n[116验收索引](../../reports/p6-output-recovery-20261012.json)。\n")
sources = {"AGENTS.md", "Cargo.toml", "Cargo.lock", "package.json", "package-lock.json", "apps/desktop/src-tauri/tauri.conf.json", "apps/desktop/src-tauri/src/maintenance.rs", "crates/core/src/lib.rs", "docs/HANDOFF-LATEST.md", "docs/MAINTENANCE-WINDOW-V1.md", "docs/WINDOWS-UPDATE-TRANSACTION-V1.md", VOLUME, prior_path, manifest_path}
for name in ["maintenance-window", "manage-windows-update", "test-windows-package", "check-backup-interrupt-window-20261012", "check-maintenance-output-20261012", "resume-backup-controller-20261012", "resume-backup-controller2-20261012", "inspect-backup-window-20261012", "close-backup-review-fixture-20261012", "check-recovery-window-20261012", "freeze-windows-package"]:
    sources.add("scripts/" + name + ".ps1")
sources.add("scripts/freeze-output-recovery-20261012.py")
for pattern in ["p6-output-*-20261012.*", "p6-backup-controller*-20261012.*", "p6-backup-interrupt-controller[234]-20261012.log", "p6-backup-interrupt-helper1-20261012.log", "p6-backup-review-fixture-*-20261012.log", "p6-maintenance-output-capture*-20261012.log", "p6-maintenance-window-before-output-fix-20261012.ps1", "p6-backup-output-final-process-audit-20261012.json"]:
    sources.update(p.relative_to(ROOT).as_posix() for p in (ROOT / "reports").glob(pattern) if p.is_file())
directories = [original, reproduced, encoding, controller, helper, recovery]
directories += [p.relative_to(ROOT).as_posix() for p in (ROOT / "reports").glob("maintenance-output-native-*") if p.is_dir()]
directories.append("reports/package-candidate-native-ab807dc686944779b4cf55d214834a24")
for directory in directories:
    for path in (ROOT / directory).rglob("*"):
        if path.is_file() and path.suffix in {".json", ".txt", ".stdout", ".stderr", ".log", ".ps1"}:
            sources.add(path.relative_to(ROOT).as_posix())
records = []
for relative in sorted(sources):
    source = ROOT / relative
    destination = ROOT / ARCHIVE / relative
    destination.parent.mkdir(parents=True, exist_ok=True)
    with destination.open("xb") as out:
        out.write(source.read_bytes())
    value = digest(source)
    assert digest(destination) == value
    records.append({"source": relative, "snapshot": destination.relative_to(ROOT).as_posix(), "sha256": value})
report = {"batch": "116", "at": at, "status": "maintenance-output-fixed-and-018-upgrade-recovery-accepted", "goalStatus": "active", "fullP3Accepted": False, "fullP4Accepted": False, "fullP5Accepted": False, "fullP6Accepted": False, "releaseAccepted": False, "accepted": accepted, "original115HistoricalLifecycleAccepted": False, "originalProductFailure": reproduced + "/original-window-failure-inspection.json", "controlledFailuresRetained": True, "interruptionCoreVersion": "0.1.7", "interruptionTools": "fixed working scripts; pinned in each session.json", "candidateVersion": "0.1.8", "packageManifest": manifest_path, "packageManifestSha256": digest(ROOT / manifest_path), "backupTestsPassed": 26, "backupTestsIgnored": 1, "desktopStagingTestsPassed": 2, "typescriptViteReleaseNsisBuildPassed": True, "previousWorkspaceRegressionReused": "112: 511 passed, 177 ignored; strict clippy", "currentCleanup": cleanup, "previousIndex": prior_path, "previousIndexSha256": digest(ROOT / prior_path), "previousArchiveRecordsVerified": len(prior["records"]), "handoffVolume": VOLUME, "modelCalls": 0, "userDesktopActions": 0, "remaining": prior["remaining"], "records": records}
(ROOT / INDEX).write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"index": INDEX, "records": len(records), "packageManifestSha256": report["packageManifestSha256"], "fullP6Accepted": False}))
