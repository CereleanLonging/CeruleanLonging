"""Real isolated core quiescence + CLI recovery; no provider or user application."""
from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent.parent
RUN = ROOT / "reports/recovery-holds-122-cli-cases"
NEW = ROOT / "target/debug/cerulean-core.exe"
OLD_PACKAGE = ROOT / "reports/p6-windows-package-018-20261012"
OLD = OLD_PACKAGE / "cerulean-runtime.exe"
assert sys.argv[1:] in ([], ["--resume"])
RESUME = sys.argv[1:] == ["--resume"]
calls = [json.loads(p.read_text(encoding="utf-8")) for p in sorted(RUN.glob("call-*.json"))] if RESUME else []
prior_servers = [json.loads(p.read_text(encoding="utf-8")) for p in sorted(RUN.glob("server-*.json"))] if RESUME else []
servers = []


def write_new(path, value):
    with Path(path).open("x", encoding="utf-8") as f:
        json.dump(value, f, ensure_ascii=False, indent=2)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def snapshot(root):
    return {p.relative_to(root).as_posix(): sha(p) for p in sorted(root.rglob("*")) if p.is_file()}


def call(binary, args, expected=0):
    proc = subprocess.Popen([str(binary), *map(str, args)], stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            creationflags=subprocess.CREATE_NO_WINDOW)
    timed_out = False
    try:
        out, err = proc.communicate(timeout=25)
    except subprocess.TimeoutExpired:
        timed_out = True
        proc.kill()
        out, err = proc.communicate(timeout=10)
    record = dict(pid=proc.pid, binary=str(binary), arguments=list(map(str, args)),
                  exitCode=proc.returncode, timeout=timed_out, waited=True, stdoutEOF=True, stderrEOF=True,
                  stdout=out.decode("utf-8", errors="replace"), stderr=err.decode("utf-8", errors="replace"))
    calls.append(record)
    write_new(RUN / f"call-{len(calls):03}.json", record)
    assert not timed_out, "Owned helper timeout, failure preserved, no automatic retry"
    if expected != "any":
        assert (proc.returncode != 0 if expected is None else proc.returncode == expected), record
    return record


def cli(data, args, expected=0, binary=NEW):
    record = call(binary, ["--workbench", "--data-dir", data, *args], expected)
    if expected is None:
        return record
    value = json.loads(record["stdout"])
    if expected == 0:
        assert value.get("ok") is True, value
    return value


def quiesce(data):
    server = subprocess.Popen([str(NEW), "--serve", "--data-dir", str(data)], stdin=subprocess.DEVNULL,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW)
    servers.append(server)
    deadline = time.monotonic() + 20
    while True:
        assert server.poll() is None, "Owned server exited before readiness; preserve original data"
        status = cli(data, ["status"], "any")
        if status.get("ok"):
            assert status["serverPid"] == server.pid
            break
        assert status.get("error", {}).get("code") == "connectionFailed", status
        assert time.monotonic() < deadline
        time.sleep(0.15)
    review = cli(data, ["upgrade", "review"])["upgradeReview"]
    assert review["ready"] and review["serverPid"] == server.pid
    prepared = cli(data, ["upgrade", "prepare", "--review-digest", review["digest"]])
    assert prepared["stopping"] and prepared["upgradeHold"]["validRecord"]
    out, err = server.communicate(timeout=20)
    receipt = dict(pid=server.pid, data=str(data), exitCode=server.returncode, waited=True,
                   stdoutEOF=True, stderrEOF=True, stdout=out.decode("utf-8"), stderr=err.decode("utf-8"),
                   automaticRestart=False, hold=prepared["upgradeHold"])
    write_new(RUN / f"server-{len(prior_servers) + len(servers):03}.json", receipt)
    assert server.returncode == 0 and not err, receipt
    return prepared["upgradeHold"]["holdDigest"]


def main():
    assert not (RUN / "result.json").exists(), "Preserve prior evidence"
    manifest = json.loads((OLD_PACKAGE / "manifest.json").read_text(encoding="utf-8-sig"))
    assert sha(OLD) == manifest["coreSha256"]
    new_hash, old_hash = sha(NEW), sha(OLD)
    build = json.loads(call(NEW, ["--build-info"])["stdout"])
    assert build["dataRecoverySchema"] == 2 and build["dataReaderGeneration"] == 3
    results = []
    for index, case in enumerate(json.loads((RUN / "cases.json").read_text(encoding="utf-8"))):
        data = Path(case["data"])
        Path(str(data).removeprefix("\\\\?\\")).resolve().relative_to(RUN.resolve())
        resuming_original = RESUME and index == 0
        if resuming_original:
            original_reviews = [v for v in calls if v["arguments"][3:5] == ["recovery", "review-previous"]]
            assert len(original_reviews) == 1 and original_reviews[0]["exitCode"] == 0
            reviewed = json.loads(original_reviews[0]["stdout"])["result"]
            current = reviewed["holdDigest"]
            assert len(prior_servers) == 1 and prior_servers[0]["exitCode"] == 0
            assert prior_servers[0]["hold"]["holdDigest"] == current
            before = snapshot(data)
            assert before == {v["path"]: v["sha256"] for v in reviewed["originalEntries"] if not v["directory"]}
            prepared = cli(data, ["recovery", "status"])["result"]
            assert prepared["reviewDigest"] == reviewed["digest"]
            assert cli(data, ["upgrade", "status"])["result"]["holdDigest"] == current
            write_new(RUN / "resume-original-review.json", dict(reviewDigest=reviewed["digest"],
                      sourceUnchanged=True, originalServerExited=True, prepareRepeated=False))
        else:
            current = quiesce(data)
            assert current != case["backupHoldDigest"]
            before = snapshot(data)
            scope = ["--from", case["backup"], "--manifest-digest", case["manifestDigest"],
                     "--hold-digest", current, "--backup-hold-digest", case["backupHoldDigest"]]
            reviewed = cli(data, ["recovery", "review-previous", *scope])["result"]
            assert reviewed["schema"] == 2 and reviewed["previousHold"]["digest"] == case["backupHoldDigest"]
            assert snapshot(data) == before
            prepared = cli(data, ["recovery", "prepare-previous", *scope, "--review-digest", reviewed["digest"]])["result"]
        assert prepared["originalState"] == "original" and prepared["stagedBackup"]
        assert snapshot(data) == before
        tx = Path(reviewed["transaction"])
        tx_before = snapshot(tx)
        # Actual frozen reader must not adopt the new transaction, even on a
        # mutation verb. No fake old-reader implementation substitutes for it.
        for args in [["recovery", "status"], ["recovery", "apply", "--review-digest", reviewed["digest"]]]:
            if resuming_original and args == ["recovery", "status"]:
                rejected = next(v for v in calls if v["binary"] == str(OLD) and v["arguments"][3:] == args)
                assert rejected["exitCode"] != 0 and rejected["waited"] and not rejected["timeout"]
            else:
                rejected = cli(data, args, None, OLD)
            assert "previousHold" in rejected["stdout"] + rejected["stderr"], rejected
            assert snapshot(tx) == tx_before and snapshot(data) == before
        if case["mode"] == "abandon":
            archived = cli(data, ["recovery", "abandon", "--review-digest", reviewed["digest"]])["result"]
            assert snapshot(data) == before
            assert cli(data, ["upgrade", "status"])["result"]["holdDigest"] == current
            retained = current
        else:
            restored = cli(data, ["recovery", "apply", "--review-digest", reviewed["digest"]])["result"]
            assert restored["restorationComplete"] and restored["restoredHoldDigest"] == case["backupHoldDigest"]
            assert snapshot(data) == snapshot(Path(case["backup"]) / "payload")
            assert snapshot(tx / "preserved") == before
            observed = cli(data, ["recovery", "status"])["result"]
            assert observed["restorationComplete"]
            archived = cli(data, ["recovery", "finish", "--review-digest", reviewed["digest"]])["result"]
            assert snapshot(Path(archived) / "preserved") == before
            retained = case["backupHoldDigest"]
            assert not (data / "events/00000000.reader3.json").exists()
        inspection = cli(data, ["recovery", "inspect", "--review-digest", reviewed["digest"]])["result"]
        assert not inspection["guardPresent"] and any(v["verified"] for v in inspection["archives"])
        cli(data, ["upgrade", "release", "--hold-digest", retained])
        fmt = cli(data, ["data-format", "status"])["result"]
        assert fmt["minimumReaderGeneration"] == (3 if case["mode"] == "abandon" else case["schema"])
        if case["mode"] == "restore":
            old_snapshot = snapshot(data)
            call(OLD, ["--stdio", "--data-dir", data])
            assert snapshot(data) == old_snapshot
        results.append(dict(schema=case["schema"], mode=case["mode"], currentHold=current,
                            backupHold=case["backupHoldDigest"], reviewDigest=reviewed["digest"],
                            archive=archived, oldControllerRejected=True, originalBytesPreserved=True,
                            actualQuiescence=True, automaticRestart=False, replayedTasks=0))
    assert sha(NEW) == new_hash and sha(OLD) == old_hash
    write_new(RUN / "result.json", dict(at=datetime.now(timezone.utc).isoformat(), status="passed", cases=results,
              newRuntimeSha256=new_hash, oldRuntimeSha256=old_hash, buildInfo=build,
              calls=len(calls), servers=len(prior_servers) + len(servers), businessModelCalls=0, userApplicationActions=0,
              originalPreparedTransactionResumed=RESUME, originalPrepareRepeated=False,
              limits=["Backup/promotion fixtures prepared by test-only helper; current quiescence and all recovery actions use actual production CLI",
                      "No native maintenance-window or installed React flow, new crash/shared-lock injection, or physical storage failure proof",
                      "Current development version unpublished; no installer or release package changed"], fullP6Accepted=False))


if __name__ == "__main__":
    try:
        main()
    finally:
        # Record live handles instead of assuming a timeout means stopped. The
        # successful path stops each exact child through upgrade preparation.
        write_new(RUN / ("owned-server-observation-resume.json" if RESUME else "owned-server-observation.json"),
                  [{"pid": p.pid, "exitCode": p.poll()} for p in servers])
