"""Owned CLI fixtures only. Preserve every process receipt and original package."""
from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parent.parent
RUN = ROOT / "reports/reader-generation-121-cli-cases"
NEW = ROOT / "target/debug/cerulean-core.exe"
OLD_PACKAGE = ROOT / "reports/p6-windows-package-018-20261012"
OLD = OLD_PACKAGE / "cerulean-runtime.exe"


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_new(path, value):
    with Path(path).open("x", encoding="utf-8") as f:
        json.dump(value, f, ensure_ascii=False, indent=2)


def snapshot(root):
    return {str(p.relative_to(root)): digest(p) for p in sorted(root.rglob("*")) if p.is_file()}


calls = []


def call(binary, args, expected=0):
    proc = subprocess.Popen([str(binary), *map(str, args)], stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            creationflags=subprocess.CREATE_NO_WINDOW)
    timed_out = False
    try:
        out, err = proc.communicate(timeout=20)
    except subprocess.TimeoutExpired:
        timed_out = True
        proc.kill()  # This exact owned child only, never a user application.
        out, err = proc.communicate(timeout=10)
    record = dict(pid=proc.pid, binary=str(binary), arguments=list(map(str, args)),
                  exitCode=proc.returncode, timeout=timed_out, waited=True,
                  stdoutEOF=True, stderrEOF=True,
                  stdout=out.decode("utf-8", errors="replace"),
                  stderr=err.decode("utf-8", errors="replace"))
    calls.append(record)
    write_new(RUN / f"call-{len(calls):03}.json", record)
    assert not timed_out, "Owned child timeout; no automatic retry"
    assert (proc.returncode == expected if expected is not None else proc.returncode != 0), record
    return record


def cli(data, args, expected=0):
    result = call(NEW, ["--workbench", "--data-dir", data, *args], expected)
    if expected is None:
        return result
    response = json.loads(result["stdout"])
    assert not response.get("error"), response
    return response["result"]


def main():
    assert not (RUN / "result.json").exists(), "Preserve prior results"
    manifest = json.loads((OLD_PACKAGE / "manifest.json").read_text(encoding="utf-8-sig"))
    assert digest(OLD) == manifest["coreSha256"]
    old_hash, new_hash = digest(OLD), digest(NEW)
    info = json.loads(call(NEW, ["--build-info"])["stdout"])
    assert info["dataReaderGeneration"] == 3
    evidence = []
    for case in json.loads((RUN / "cases.json").read_text(encoding="utf-8")):
        data = Path(case["data"])
        data.resolve().relative_to(RUN.resolve())
        scope = ["--from", case["backup"], "--manifest-digest", case["manifestDigest"],
                 "--hold-digest", case["holdDigest"]]
        before = snapshot(data)
        review = cli(data, ["data-format", "review-reader", *scope])
        assert snapshot(data) == before
        cli(data, ["data-format", "promote-reader", *scope, "--review-digest", "0" * 64], None)
        assert snapshot(data) == before
        promoted = cli(data, ["data-format", "promote-reader", *scope, "--review-digest", review["digest"]])
        assert promoted["minimumReaderGeneration"] == 3 and promoted["dataSchema"] == case["schema"]
        after = snapshot(data)
        assert all(after.get(p) == value for p, value in before.items())
        assert set(after) - set(before) == {"events\\00000000.reader3.json"}
        # Release is an explicit separate action; otherwise an old reader could
        # be rejecting the hold instead of the new generation guard.
        cli(data, ["upgrade", "release", "--hold-digest", case["holdDigest"]])
        released = snapshot(data)
        for mode in ["--stdio", "--serve"]:
            rejected = call(OLD, [mode, "--data-dir", data], None)
            assert "missing field" in rejected["stderr"] and "body" in rejected["stderr"], rejected
            assert snapshot(data) == released
        call(NEW, ["--stdio", "--data-dir", data])
        assert snapshot(data) == released
        evidence.append(dict(schema=case["schema"], reviewDigest=review["digest"],
                             minimumReaderGeneration=3, originalBytesPreserved=True,
                             legacyStdioRejected=True, legacyServiceRejected=True,
                             newStdioRead=True, afterRelease=released))
    assert digest(OLD) == old_hash and digest(NEW) == new_hash
    write_new(RUN / "result.json", dict(at=datetime.now(timezone.utc).isoformat(), status="passed",
              oldRuntimeSha256=old_hash, newRuntimeSha256=new_hash, newBuildInfo=info,
              cases=evidence, calls=len(calls), modelCalls=0, userDesktopActions=0,
              installed=False, packagePublished=False, fullP6Accepted=False,
              limits=["Synthetic held cold-backup fixtures; production CLI actions afterward",
                      "No GUI/update transaction, crash injection or post-release backup recovery proof",
                      "Current source version is unpublished; frozen 0.1.8 is preserved"]))


if __name__ == "__main__":
    main()
