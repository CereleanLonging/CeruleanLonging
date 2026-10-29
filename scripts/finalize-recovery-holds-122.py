"""Record the verified recovery delta; preserve old evidence and concurrent edits."""
from pathlib import Path
from datetime import datetime, timedelta, timezone
import hashlib
import json
import re
import subprocess

root = Path(__file__).resolve().parent.parent
now = datetime.now(timezone(timedelta(hours=8)))
at = now.isoformat(timespec="seconds")


def load(path):
    return json.loads((root / path).read_text(encoding="utf-8-sig"))


def sha(path):
    return hashlib.sha256((root / path).read_bytes()).hexdigest()


def record(path):
    return dict(path=Path(path).as_posix(), bytes=(root / path).stat().st_size, sha256=sha(path))


def write_json(path, value):
    with (root / path).open("x", encoding="utf-8") as f:
        json.dump(value, f, ensure_ascii=False, indent=2)
        f.write("\n")


baseline = load("reports/recovery-holds-122-baseline.json")
prior = load("reports/acceptance-121-20261013.json")
for evidence in prior["evidence"]:
    assert record(evidence["path"]) == evidence, evidence["path"]
head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
assert head == baseline["head"]
changed = [p for p, old in baseline["files"].items() if not (root / p).is_file() or sha(p) != old]
assert set(changed) == {"crates/core/src/backup_recovery.rs", "crates/core/src/upgrade.rs",
                        "crates/core/src/workbench_cli.rs", "crates/core/src/main.rs"}, changed
log = (root / "reports/recovery-holds-122-workspace.log").read_text(encoding="utf-8-sig")
groups = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored", log)
assert len(groups) == 22 and [sum(int(v[i]) for v in groups) for i in range(3)] == [521, 0, 180]
assert "test result: FAILED" not in log
assert "17 passed; 0 failed; 1 ignored" in (root / "reports/recovery-holds-122-unit-1.log").read_text(encoding="utf-8-sig")
assert "Finished `dev` profile" in (root / "reports/recovery-holds-122-clippy.log").read_text(encoding="utf-8-sig")
assert (root / "reports/recovery-holds-122-fmt.log").stat().st_size == 0
cli = load("reports/recovery-holds-122-cli-cases/result.json")
assert cli["status"] == "passed" and cli["calls"] == 44 and cli["servers"] == 3
assert cli["originalPreparedTransactionResumed"] and not cli["originalPrepareRepeated"]
assert sha("target/debug/cerulean-core.exe") == cli["newRuntimeSha256"]
calls = sorted((root / "reports/recovery-holds-122-cli-cases").glob("call-*.json"))
assert len(calls) == 44
for path in calls:
    value = json.loads(path.read_text(encoding="utf-8"))
    assert value["waited"] and value["stdoutEOF"] and value["stderrEOF"] and not value["timeout"]
    if value["exitCode"]:
        assert "previousHold" in value["stdout"] + value["stderr"]
server_files = sorted((root / "reports/recovery-holds-122-cli-cases").glob("server-*.json"))
for path in server_files:
    value = json.loads(path.read_text(encoding="utf-8"))
    assert value["exitCode"] == 0 and value["waited"] and value["stdoutEOF"] and value["stderrEOF"]
assert not load("reports/recovery-holds-122-processes.json")["matches"]
signature = load("reports/signature-close-consistency-122-20261013.json")
assert signature["priorOfflineUnsignedResultsReusableForSameBytes"]
for value in signature["artifactPaths"]:
    assert sha(value["path"]) == value["expectedSha256"]
for value in signature["manifests"]:
    assert sha(value["path"]) == value["expectedSha256"]

sources = [p for p in baseline["files"] if p.startswith(("crates/", "apps/", "scripts/"))
           or p in ("Cargo.toml", "Cargo.lock", "package.json", "package-lock.json")]
sources += ["scripts/check-previous-recovery-122.py", "scripts/finalize-recovery-holds-122.py"]
write_json("reports/recovery-holds-122-source-readback.json", dict(at=at, head=head,
           changedTrackedFiles=changed, files=[record(p) for p in sources], priorEvidenceUnchanged=37,
           limitation="Current source snapshot after verification, not an atomic/reproducible build proof"))

evidence_names = ["recovery-holds-122-baseline.json", "recovery-holds-122-unit-1.log", "recovery-holds-122-workspace.log",
    "recovery-holds-122-build.log", "recovery-holds-122-export.log", "recovery-holds-122-clippy.log", "recovery-holds-122-fmt.log",
    "recovery-holds-122-processes.json", "read-recovery-holds-122-processes.ps1", "check-previous-recovery-122-first-run.py",
    "recovery-holds-122-source-readback.json", "signature-close-consistency-122-20261013.json", "snapshot-signature-close-122-20261013.py"]
evidence_names += [p.relative_to(root / "reports").as_posix() for p in sorted((root / "reports/recovery-holds-122-cli-cases").glob("*.json"))]
remaining = [
    "Reader-aware binary update/rollback capability validation, manifest capability comparison and old-controller exclusion remain",
    "Maintenance window previous-backup recovery and reader promotion controls, plus installed React entry, remain",
    "Schema-2 actual crash/shared-lock/finalization-loss differences and full physical storage/load matrix remain; inherited schema-1 checks do not prove all new paths",
    "Unique candidate version, backup writer allowlist and actual candidate/runtime/manifest/signature consistency remain before release",
    "All other P3/P4/P5/P6 obligations retained from formal-acceptance-gates-120; no user scope removed",
]
report = dict(schema=1, at=at, item=122, status="progress", goalStatus="active", baselineGitHead=head,
    acceptedScope="Explicit previous-backup recovery with distinct current/backup holds and original release proof; production CLI quiescence/recovery/abandonment and frozen old-controller rejection",
    sourceModified=True, newReleasePackageBuilt=False, currentDeclaredVersion="0.1.8", frozenLatestVersion="0.1.8",
    compatibility=dict(dataRecoverySchema=2, legacyRecoverySchema1Readable=True, legacyReviewBytesPreservedWhenPreviousHoldAbsent=True,
                       reviewedNewTransactionsRejectOldReader=True, immutableBackupNotRewritten=True,
                       currentDirectoryPreservedOnRestore=True, automaticRestart=False, automaticReplay=False),
    checks=dict(workspace=dict(groups=22, passed=521, failed=0, ignored=180), recoveryUnit=dict(passed=17, failed=0, ignored=1),
                clippyExitCode=0, fmtExitCode=0, cli=cli, ownedResidualProcesses=0),
    signing=dict(closeReport="reports/signature-close-consistency-122-20261013.json", enumeratedArtifacts=44, manifests=10,
                 newArtifacts=0, newManifests=0, codeSigningCertificateFingerprintsUnchanged=True,
                 matchingTrustLocationsUnchanged=True, priorOfflineAudit="reports/signature-offline-audit-121-final-20261013.json",
                 unchangedBytesRetainUnsignedResult=True, formalPublisherAccepted=False, onlineRevocationVerified=False,
                 noKeyCreation=True, noTrustChange=True, noExternalTransfer=True),
    preservedFailures=[dict(kind="verification harness channel assumption", firstExitCode=1,
        originalServerPid=5192, originalServerExitCode=0, originalOldReaderPid=13292,
        outcome="Old reader correctly rejected previousHold on stderr; original harness looked only at stdout",
        recovery="Original prepared transaction read back and resumed; prepare not repeated; original script/calls/observation preserved")],
    fullP3Accepted=False, fullP4Accepted=False, fullP5Accepted=False, fullP6Accepted=False, releaseAccepted=False,
    businessModelCalls=0, userApplicationActions=0, commitOrPushPerformed=False, limitations=remaining,
    next="Wire reader/update capabilities and transaction isolation, then maintenance GUI; reuse the completed core/CLI recovery proofs",
    evidence=[record("reports/" + p) for p in evidence_names])
write_json("reports/acceptance-122-20261013.json", report)

body = f"""<!-- acceptance-122-start -->
当前：{at}，卷004第001项（122），progress。解除原hold后的旧备份恢复核心与实际CLI限定范围通过；源码未发行，完整P3/P4/P5/P6与releaseAccepted仍false，目标active。

已修复121确认的缺口：新增 `recovery review-previous/prepare-previous`，显式提交当前hold和备份旧hold两个摘要。原备份必须通过完整清单/字节/原位置/SID校验；当前目录须有有效新hold及旧hold原解除归档。两份hold的内容、释放记录、当前完整目录清单都纳入审阅摘要，拒绝缺失/变动/过期审阅。恢复不覆盖备份、不重建旧hold；保留当前完整目录后复制旧备份，完成时仍持备份原hold；放弃尚未开始的准备则保留当前hold与新数据。未知不自动重放或启动。

新事务schema2包含previousHold；旧schema1字段缺省时不写入新字段，旧摘要顺序保持，新核心继续读取旧事务。build-info dataRecoverySchema=2，程序更新协议尚待相应接线。新恢复状态分别返回originalHoldDigest/restoredHoldDigest，收尾按实际保留的hold核对。没有删guard、删监督字段或伪造旧hold来绕过读取保护。

验收：恢复专项17通过/1默认ignored；workspace22组521 passed/0 failed/180默认ignored；严格clippy与fmt通过。实际3个自有驻留核心经生产CLI审阅、生成真实当前hold并停止（PID5192、23896、19428，均exit0/双EOF）。两例schema1/2恢复成功，完整当前目录字节保存在归档preserved，恢复字节精确等于原backup payload；明确结束并释放备份hold后，冻结0.1.8 stdio可读。另一例准备后放弃保持新数据与当前hold。冻结0.1.8对三例schema2恢复事务的status/apply均因previousHold拒绝，原目录和事务字节未变。44个CLI/helper子进程回执均有实际PID、退出和双EOF，限定CIM匹配0。备份/提升夹具由测试帮助器构造，当前锁定及全部恢复动作走实际生产CLI；不是图形安装入口或完整故障矩阵。

保留验收器失败：第一次正确拒绝出现在stderr，脚本只检查stdout而退出1，第一例已准备但尚未apply。原核心已正常退出，随后只读核查同一review/hold/目录清单并续接；未重复prepare，原脚本及7个原回执保留，原old-reader PID13292不是业务模型。产品源码无需为此修改。

签名收尾重新枚举原发行位置：44产物、10清单摘要未变，无新增产物/清单；代码签名证书指纹及对应信任位置未变。因此相同字节沿用121本次离线核查的未签名结果，未据昨天结论推断新文件。当前是本地测试证书，不是正式发布者证明；没有新建密钥、改变信任或外发。当前开发runtime hash已变化且仍是未发行0.1.8，与冻结0.1.8不能混同。其他模型的修改保留，本项跟踪源码差异仅4个文件；121的37项证据复核未变。

下一阶段先补reader-aware程序更新/回退能力、清单比较和旧控制器隔离，再接维护窗口的明确提升与旧备份恢复；补新schema2中断/共享/回执故障差异。随后同步唯一候选版本和backup writer白名单，再制作新候选并按当时签名/清单实际核查。完整P3–P6其余义务仍按120门槛索引保留，默认0.1.7安装、WorkBuddy064和原型通过成果不动；不重复基础接入或本项已完成恢复动作。

[122总索引](../reports/acceptance-122-20261013.json)、[实际恢复结果](../reports/recovery-holds-122-cli-cases/result.json)、[原事务续接核对](../reports/recovery-holds-122-cli-cases/resume-original-review.json)、[签名收尾](../reports/signature-close-consistency-122-20261013.json)、[源码复核](../reports/recovery-holds-122-source-readback.json)。
<!-- acceptance-122-end -->
"""


def prepend(path, content):
    p = root / path
    original = p.read_bytes()
    assert hashlib.sha256(original).hexdigest() == baseline["files"][path], ("Concurrent edit", path)
    # Binary prefix retains historical bytes exactly, including their line endings.
    p.write_bytes(content.encode("utf-8") + b"\n\n" + original)


prepend("docs/HANDOFF-LATEST.md", "# 最新交接 / 122\n\n" + body + "\n下方121及更早为历史记录，当前状态以122为准。")
handoff = f"handoffs/2026-10-13/handoff_2026-10-13_004_{now.strftime('%H%M%S')}+0800.md"
with (root / handoff).open("xb") as f:
    f.write((f"# Handoff 122 / {at}\n\n" + body.replace("](../reports/", "](../../reports/")).encode("utf-8"))
next_stage = """# 后续验收顺序 / 122

122已补齐不同hold的旧备份恢复核心/CLI，并实测原事务续接、旧控制器拒绝和恢复后旧reader可读，见[122交接](HANDOFF-LATEST.md)与[122索引](../reports/acceptance-122-20261013.json)。不要重复121缺口验证或本项已完成恢复动作。

1. 完成reader-aware程序更新与回退检查：最低reader、容器schema、实际build-info与manifest能力比较都要验证；旧缺字段只用明确版本映射，新事务须拒绝旧维护控制器。
2. 将121的reader提升和122的previous-backup恢复接入维护窗口，区分当前与备份hold，保留明确审阅、单次确认及原事务核查；验证原生窗口和安装版React入口。
3. 只补新guard/schema2真实共享、中断、回执缺失等差异；104–119及121–122已通过机制继续复用，不能将旧故障矩阵直接代证新字段路径。
4. 再同步唯一新版本、backup writer白名单、能力清单和实际候选runtime，制作候选后重新核对签名。当前源码未发行，不能与冻结0.1.8混同。
5. 原完整P3/P4/P5/P6门槛继续保留；真实管理/编程/效率、多供应商与桌面、金额预算/隔离、独立机器安装及正式发行等仍未全量签收。签名最新见122实际收尾，未授权创建密钥、变更信任或外发；缺环境/正式发布身份先做其他工程项，不重复询问。

下方121及更早为历史执行顺序，以122为准。
"""
prepend("docs/NEXT-STAGE-GATES.md", next_stage)
prepend("docs/READER-GENERATION-NEXT.md", "# 实施进展 / 122\n\n121的解除hold后恢复缺口已由不同hold绑定、原释放归档和schema2恢复事务补齐，核心与实际CLI通过限定验收；见[122索引](../reports/acceptance-122-20261013.json)。维护窗口、程序更新/回退能力门禁、差异故障矩阵及新候选仍待完成。以下为保留的121/120记录，历史缺口状态不覆盖本项新证据。")
write_json("reports/recovery-holds-122-handoff.json", dict(at=at, handoff=handoff,
           index=record("reports/acceptance-122-20261013.json"), docs=[record(p) for p in ["docs/HANDOFF-LATEST.md", "docs/NEXT-STAGE-GATES.md", "docs/READER-GENERATION-NEXT.md", handoff]]))
print(json.dumps(dict(evidence=len(report["evidence"]), changedSourceFiles=len(changed), handoff=handoff, goal="active")))
