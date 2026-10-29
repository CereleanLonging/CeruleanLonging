"""Freeze new evidence and prepend handoff without rewriting historical reports."""
import hashlib
import json
import re
import subprocess
from datetime import datetime, timedelta, timezone
from pathlib import Path

root = Path(__file__).resolve().parent.parent
at = datetime.now(timezone(timedelta(hours=8))).isoformat(timespec="seconds")


def load(path):
    return json.loads((root / path).read_text(encoding="utf-8-sig"))


def sha(path):
    return hashlib.sha256((root / path).read_bytes()).hexdigest()


def artifact(path):
    p = root / path
    return dict(path=Path(path).as_posix(), bytes=p.stat().st_size, sha256=sha(path))


def new_json(path, value):
    with (root / path).open("x", encoding="utf-8") as f:
        json.dump(value, f, ensure_ascii=False, indent=2)
        f.write("\n")


baseline = load("reports/reader-generation-121-baseline-20261013.json")
prior = load("reports/acceptance-120-20261013.json")
for item in prior["evidence"]:
    assert artifact(item["path"]) == item, ("Previous evidence changed", item["path"])
head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
assert head == baseline["head"]
audit = load("reports/signature-offline-audit-121-final-20261013.json")
old_audit = load("reports/signature-offline-audit-120-20261013.json")
paths = [v["observed"] for p in audit["packages"] for v in p["artifacts"]] + audit["releaseArtifacts"]
old_paths = [v["observed"] for p in old_audit["packages"] for v in p["artifacts"]] + old_audit["releaseArtifacts"]
assert {(v["path"], v["sha256"]) for v in paths} == {(v["path"], v["sha256"]) for v in old_paths}
assert not audit["embeddedSignedScriptBlocks"]
for value in paths:
    assert value["unchangedDuringRead"] and sha(value["path"]) == value["sha256"]
    assert value["signature"]["pe"]["certificateTableBytes"] == 0
    assert value["signature"]["winVerifyTrustCacheOnly"] == "0x800B0100"
for package in audit["packages"]:
    assert package["manifestUnchangedDuringRead"]
    assert sha(Path(package["path"]) / "manifest.json") == package["manifestSha256"]
    for item in package["artifacts"]:
        assert item["bytesMatch"] and item["sha256Match"]
        if item["expected"]["path"] != "cerulean-runtime.exe":
            assert item["observed"]["productVersion"] == package["version"]
certs = [c["certificate"]["thumbprint"] for c in audit["certificates"]]
assert certs == [c["certificate"]["thumbprint"] for c in old_audit["certificates"]]

workspace = (root / "reports/reader-generation-121-workspace-1.log").read_text(encoding="utf-8-sig")
groups = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored", workspace)
totals = [sum(int(g[i]) for g in groups) for i in range(3)]
assert len(groups) == 22 and totals == [518, 0, 179]
assert "test result: FAILED" not in workspace
assert "5 passed; 0 failed; 1 ignored" in (root / "reports/reader-generation-121-unit-final.log").read_text(encoding="utf-8-sig")
assert "Finished `dev` profile" in (root / "reports/reader-generation-121-clippy.log").read_text(encoding="utf-8-sig")
assert (root / "reports/reader-generation-121-fmt.log").stat().st_size == 0
cli = load("reports/reader-generation-121-cli-cases/result.json")
assert cli["status"] == "passed" and len(cli["cases"]) == 2 and cli["calls"] == 15
assert sha("target/debug/cerulean-core.exe") == cli["newRuntimeSha256"]
assert not load("reports/reader-generation-121-processes.json")["matches"]
mixed = []
for adapter in ["deepseek", "omp"]:
    value = load(f"reports/p5-scoped-supervision-mixed-reader121-cases-20261013/{adapter}/result.json")
    assert value["acceptedTasks"] == 3 and value["managementRequests"] == 8
    assert value["nativeCodexTurns"] == 1 and value["steerCalls"] == 1
    assert value["promptResends"] == value["reopenNewEvents"] == value["realModelCalls"] == 0
    mixed.append({k: v for k, v in value.items() if k != "snapshot"})

changed = [p for p, old in baseline["files"].items() if not (root / p).is_file() or sha(p) != old]
expected = {"data_format.rs", "host.rs", "mixed.rs", "lib.rs", "main.rs", "management_application.rs",
            "management_execution.rs", "project_planning.rs", "project_setup.rs", "resource_config_tests.rs", "scheduler.rs", "workbench_cli.rs"}
assert {Path(p).name for p in changed} == expected, changed
sources = [p for p in baseline["files"] if p.startswith(("crates/", "apps/", "scripts/")) or p in ("Cargo.toml", "Cargo.lock", "package.json", "package-lock.json")]
sources += ["crates/core/src/reader_generation.rs", "scripts/check-reader-generation-121.py", "scripts/finalize-reader-generation-121.py"]
new_json("reports/reader-generation-121-source-readback.json", dict(at=at, head=head, changedTrackedFiles=changed,
         files=[artifact(p) for p in sources], preservedPriorEvidence=len(prior["evidence"]),
         limitation="Post-regression source snapshot, not atomic or reproducible build provenance. No source/packaging version bump this item."))

evidence_names = [
    "reader-generation-121-baseline-20261013.json", "reader-generation-121-workspace-1.log",
    "reader-generation-121-unit-1.log", "reader-generation-121-unit-2.log", "reader-generation-121-unit-3.log",
    "reader-generation-121-unit-final.log", "reader-generation-121-build.log", "reader-generation-121-export.log",
    "reader-generation-121-cli-cases/result.json", "reader-generation-121-recovery-gap.json",
    "reader-generation-121-clippy.log", "reader-generation-121-fmt.log", "reader-generation-121-processes.json",
    "reader-generation-121-source-readback.json", "p5-scoped-supervision-mixed-reader121-20261013.log",
    "p5-scoped-supervision-mixed-reader121-cases-20261013/deepseek/result.json",
    "p5-scoped-supervision-mixed-reader121-cases-20261013/omp/result.json",
    "signature-offline-audit-121-20261013.json", "signature-offline-audit-121-final-20261013.json",
    "audit-signatures-offline-121-20261013.ps1", "audit-signatures-offline-121-final-20261013.ps1",
    "read-reader-generation-121-processes.ps1",
]
evidence_names += [p.relative_to(root / "reports").as_posix() for p in sorted((root / "reports/reader-generation-121-cli-cases").glob("call-*.json"))]
remaining = [
    "Reader generation is not yet wired into program update/rollback capability checks and manifest capability-key comparison",
    "New update transaction/controller schema isolation and reviewed legacy-version mapping remain",
    "Post-release original-backup recovery fails because the original matching hold is gone; requires reviewed new mechanism, no manual guard/hold deletion or recreation",
    "Maintenance GUI reader promotion and installed full React entry not covered",
    "Reader publication crash/real sharing, lost reply, disk-full/sync-failure matrix remains",
    "Unique candidate version, backup writer allowlist, candidate build/manifest/runtime and actual signing recheck pending; current new source is unpublished 0.1.8",
    "All other full P3/P4/P5/P6 gates retained from acceptance 120 and formal-acceptance-gates-120; no scope reduction",
]
report = dict(schema=1, at=at, item=121, status="progress", goalStatus="active", baselineGitHead=head,
    acceptedScope="Core reader-generation guard, explicit backup-bound CLI promotion, pre-write/load refusal, actual frozen 0.1.8 refusal and offline signing consistency",
    sourceModified=True, newReleasePackageBuilt=False, currentDeclaredVersion="0.1.8", frozenLatestVersion="0.1.8",
    checks=dict(workspace=dict(groups=22, passed=518, failed=0, ignored=179),
                finalTargeted=dict(passed=5, failed=0, ignored=1, refinement="Three historical record shapes individually exercised after workspace regression; product code unchanged"),
                clippyExitCode=0, fmtExitCode=0, cli=cli, mixed=mixed, residualOwnedProcesses=0),
    signing=dict(freshAudit="reports/signature-offline-audit-121-final-20261013.json", packageCount=10,
                 artifactPaths=len(paths), distinctArtifactHashes=len({v['sha256'] for v in paths}),
                 allObservedArtifactsUnsigned=True, certificates=certs, certificateTrustLocations=audit["certificateTrustLocations"],
                 manifestsAndArtifactsUnchangedFrom120=True, publicPublisherAccepted=False, revocationFreshnessVerified=False,
                 privateKeysCreatedOrAccessed=False, trustChanged=False, externalTransfer=False,
                 note="Unsigned claims apply to enumerated frozen/release artifacts; debug runtime is separately identified by its build-info and hash. Runtime has no PE product version; association uses manifest hash. Existing UNK/NSS GUI normalization finding is reused for unchanged bytes."),
    preservedFailures=["unit-1: synthetic hold root was not canonical; test fixture corrected", "unit-3: snapshot attempted reading host-locked writer.lock; now presence/length compared for lock files, other file bytes compared", "first fresh signature collector matched its own SIG search literal; preserved and superseded by anchored final collector"],
    compatibility=dict(minimumReaderGeneration=3, readableContainerSchemas=[1,2], feature="Both explicit enabled and disabled supervisionOverride require promotion; None retains old bytes", postReleaseRecovery="confirmed gap; original read-only review failed without modifying data"),
    fullP3Accepted=False, fullP4Accepted=False, fullP5Accepted=False, fullP6Accepted=False, releaseAccepted=False,
    businessModelCalls=0, userApplicationActions=0, commitOrPushPerformed=False,
    limitations=remaining, next="Complete reader-aware update protocol and post-release backup recovery before any new release candidate; preserve existing evidence and other model changes",
    evidence=[artifact("reports/" + p) for p in evidence_names])
new_json("reports/acceptance-121-20261013.json", report)

body = f"""<!-- acceptance-121-start -->
当前：{at}，卷003第010项（121），progress；核心读取代际与当前签名一致性限定范围通过。源码未发行，完整 P3/P4/P5/P6 与 releaseAccepted 均为 false，目标 active。

本项先反查120和读取代际预案，保留其他模型的监督、UI、安装驱动与原型成果。新增不可变 `events/00000000.reader3.json`，独立表达 minimumReaderGeneration=3，容器仍为 schema 1/2。两类显式 supervisionOverride 在配置、直接方案、管理应用和工作流写入前检查；无保护的旧目录拒绝写入，已有新字段却缺保护时拒绝加载或事后自动补证。None 保持旧字节。离线 `data-format review-reader/promote-reader` 复用原 hold、冷备份、摘要绑定审阅、禁止覆盖发布；不改写旧 marker，不自动解除 hold。未知/损坏/迁址/未完成保护、活动写入者、程序更新占用或源文件变化均拒绝。维护窗口与程序更新协议尚未接入此能力。

实际验收：workspace 22组，518 passed / 0 failed / 179默认ignored；最终三类历史记录各自补测后的专项5通过/1默认ignored；严格clippy与fmt通过。两个schema的实际CLI提升在错误审阅摘要时零写入，正确提升只新增保护文件。明确解除hold后，冻结0.1.8的stdio与serve均因缺少事件body拒绝加载；当前开发核心stdio可读，原文件摘要不变。15次自有子进程均有实际PID、退出及双EOF回执；无超时。两个受控Codex+DeepSeek/OMP场景各3任务Accepted、8次合成管理请求、1原轮次/1建议、0重发/0重开新增事件。UI源码未改，复用120的15界面场景，不代证当前完整安装桌面。最终限定目录CIM matches=[]，未停止用户进程。

重新核查的签名状态：同一张CurrentUser\\My本地测试用途证书，指纹`{certs[0]}`；所查10份冻结包30产物及14个release/sidecar路径，共31种字节，均无PE嵌入签名，离线WinVerifyTrust为0x800B0100。清单、长度、GUI/setup版本和runtime摘要一致，产物与120未变；正式发布者身份和在线吊销未通过。首次本轮收集器复现了搜索字符串误报签名块，原报告保留，以锚定整行的final审计为准，真实脚本签名块0。没有新建密钥、使用私钥、变更信任或外发。当前开发runtime同为未发行0.1.8但能力新增，不与冻结0.1.8混同；必须同步唯一新版本及能力清单后才能出候选。

新发现并实测保留的缺口：解除提升hold后，原冷备份的恢复审阅在两个schema均失败，且零写入。现有恢复要求活动目录与备份包含完全相同的有效hold；不能手工补回旧hold、删guard或删字段来伪造兼容。后续先设计并实现明确审阅、绑定新活动hold与旧备份证明的恢复流程，再接升级/回退门禁与维护窗口。当前不宣称完整升级回退可用。

其余未验收范围继续按120正式门槛索引：真实管理与编程/效率、原首批多供应商与桌面产品路径、金额预算/隔离、安装入口/独立Windows/正式发布签名及完整故障矩阵。默认0.1.7安装保留。没有真实供应商调用、用户应用操作或提交推送，WorkBuddy064未重发。旧证据26项摘要复核未变，新增跟踪修改仅12个计划内Rust文件及新模块/验收工具；源快照不等于可复现构建证明。

[121总索引](../reports/acceptance-121-20261013.json)、[真实CLI结果](../reports/reader-generation-121-cli-cases/result.json)、[恢复缺口](../reports/reader-generation-121-recovery-gap.json)、[当前签名审计](../reports/signature-offline-audit-121-final-20261013.json)、[源码复核](../reports/reader-generation-121-source-readback.json)。
<!-- acceptance-121-end -->"""


def prepend(path, title, text):
    p = root / path
    original = p.read_bytes()
    assert hashlib.sha256(original).hexdigest() == baseline["files"][path], ("Concurrent doc edit", path)
    value = original.decode("utf-8-sig")
    p.write_text(title + "\n\n" + text + "\n\n下方120及更早为保留历史，当前状态以121为准。\n\n" + value, encoding="utf-8")


prepend("docs/HANDOFF-LATEST.md", "# 最新交接 / 121", body)
prepend("handoffs/2026-10-13/handoff_2026-10-13_003_020902+0800.md", f"# Handoff 121 / {at}", body.replace("](../reports/", "](../../reports/"))
next_text = """121：核心读取代际及明确CLI提升限定验收通过；当前源码未发行，完整门槛未完成。

1. 先读[121交接](HANDOFF-LATEST.md)与[121索引](../reports/acceptance-121-20261013.json)，复用已通过核心与原型，不重跑基础接入。
2. 优先补解除hold后的旧备份恢复：本项已实际证实原流程拒绝。禁止补造hold、删除guard或删新字段；需摘要绑定的新旧hold/备份审阅机制，并处理原事务与回执不确定性。
3. 完成reader-aware更新/回退检查、能力清单比较与旧版本明确映射，新事务不被旧控制器接管；代际提升必须在程序事务结束后。接维护窗口审阅、明确提升与恢复入口。
4. 补新保护发布中断/真实共享/丢失回执；已有104–119机制证据继续保留，不代证新增路径。
5. 再同步唯一候选版本、backup writer白名单、build-info/实际runtime/清单；重新核查当时证书及签名。当前本地测试证书与44条未签名产物是本轮实测，不沿用昨天或声称正式签名；不创建密钥、改变信任或外发。
6. 完整P3–P6其余门槛仍继承120索引。暂缺独立环境/正式发布身份继续工程工作，不重复问确认；默认0.1.7安装和WorkBuddy064保留。
"""
prepend("docs/NEXT-STAGE-GATES.md", "# 后续验收顺序 / 121", next_text)
plan = root / "docs/READER-GENERATION-NEXT.md"
assert sha("docs/READER-GENERATION-NEXT.md") == baseline["files"]["docs/READER-GENERATION-NEXT.md"]
old_plan = plan.read_text(encoding="utf-8")
plan.write_text("# 读取代际实施状态 / 121\n\n核心guard、首写入/重开检查及明确CLI提升已实现并通过限定验收，见[121索引](../reports/acceptance-121-20261013.json)。程序更新协议、能力清单、维护窗口、发布故障矩阵及唯一新候选版本仍待完成。解除hold后恢复原冷备份已确认存在缺口；不能将本阶段写成完整回退验收。当前源码仍未发行，未升级用户数据。\n\n以下为120时的设计记录，保留原文；已完成与剩余以121交接为准。\n\n" + old_plan, encoding="utf-8")
print(json.dumps(dict(report="reports/acceptance-121-20261013.json", evidence=len(report["evidence"]),
                      trackedChanges=len(changed), preservedPriorEvidence=len(prior["evidence"]), goal="active")))
