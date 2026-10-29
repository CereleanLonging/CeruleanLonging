"""Stage a frozen 239 slot for the existing Host; never dispatch or approve."""
from pathlib import Path, PureWindowsPath
import argparse
import hashlib
import json
import os
import stat

ROOT = Path(__file__).resolve().parents[1]
PINS = {
    "workloads": ("reports/efficiency-preparation-239/boundary-corrections/workloads.json",
                  "6a35106045fc2affb9810a605fe22b82fb3a90b5ad30415e1df53fa85d9e13c2"),
    "schedule": ("reports/efficiency-preparation-239/boundary-corrections/schedule.json",
                 "3e5cd950221fcb3dbf0d67d4719e7c18ead737e76547e4dbf6162cdd16612391"),
    "checker": ("reports/efficiency-preparation-239/verify.mjs",
                "5056b67d9ba932c784c9625a1fd457ae3cb83e2aaf70dfd9300171302103e536"),
    "launcher": ("reports/efficiency-host-240/product-verifier.mjs",
                 "a818acfbe45f361890872bf82f190ee957afc7ee09abe879f76fb9f46ed11f06"),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def safe_relative(value):
    require(isinstance(value, str) and value, "Empty relative path")
    parts = value.split("/")
    win = PureWindowsPath(value)
    require(not win.drive and not win.root and "\\" not in value,
            "Absolute or Windows-special path")
    require(all(p not in ("", ".", "..") and ":" not in p
                and not p.endswith((" ", ".")) for p in parts), "Unsafe path component")
    require(not win.is_reserved(), "Reserved path")
    return Path(*parts)


def no_links(path):
    """Reject existing junctions/reparse points as well as symbolic links."""
    path = Path(os.path.abspath(path))
    for part in [*reversed(path.parents), path]:
        try:
            meta = part.lstat()
        except FileNotFoundError:
            continue
        require(not stat.S_ISLNK(meta.st_mode)
                and not getattr(meta, "st_file_attributes", 0)
                & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400),
                "Link/reparse point is not allowed")
    return path


def checked_read(root, relative, digest):
    path = no_links(root / safe_relative(relative))
    data = path.read_bytes()
    require(sha(data) == digest, "Frozen source digest mismatch: " + relative)
    return data


def load_suite(root=ROOT):
    root = no_links(root)
    pinned = {name: checked_read(root, rel, digest)
              for name, (rel, digest) in PINS.items()}
    workloads = json.loads(pinned["workloads"])["workloads"]
    schedule = json.loads(pinned["schedule"])
    require(len(workloads) == 12 and len(schedule["slots"]) == 72
            and schedule["authorized"] is False, "Unexpected frozen suite")
    return root, workloads, schedule


def write_new(path, data):
    with path.open("xb") as stream:
        stream.write(data)


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")


def prepare(slot_id, destination, root=ROOT):
    root, workloads, schedule = load_suite(root)
    matches = [s for s in schedule["slots"] if s["id"] == slot_id]
    require(len(matches) == 1, "Unknown or ambiguous slot")
    slot = matches[0]
    require(slot["status"] == "not-started" and slot["authorized"] is False,
            "Slot is not an unstarted preparation")
    workload = next(w for w in workloads if w["id"] == slot["task"])
    require(all(slot[k] == workload[k] for k in
                ("seed", "checkerSha256", "taskSha256")), "Slot/workload mismatch")
    fixture = safe_relative(workload["fixtureDirectory"])
    require(fixture.is_relative_to(Path("reports/efficiency-preparation-239")),
            "Fixture outside frozen suite")
    # Only these bytes are read: no reference, mutant, or 240 prepared.json.
    files = {}
    for name, digest in workload["seed"].items():
        require(name in ("main.mjs", "left.mjs", "right.mjs"), "Unexpected artifact")
        files[name] = checked_read(root, (fixture / "seed" / name).as_posix(), digest)
    task = checked_read(root, (fixture / "task.txt").as_posix(), workload["taskSha256"])
    spec = checked_read(root, (fixture / "verify.json").as_posix(), workload["checkerSha256"])

    destination = no_links(destination)
    source_suite = root / "reports/efficiency-preparation-239"
    require(not destination.is_relative_to(source_suite)
            and not source_suite.is_relative_to(destination), "Destination overlaps source")
    require(not destination.exists(), "Preserve existing attempt; choose a fresh directory")
    require(destination.parent.is_dir(), "Destination parent must already exist")
    # Exclusive directory creation reserves an attempt. A partial directory is preserved
    # on failure, and ready.json is published last. No implicit cleanup or resume.
    destination.mkdir()
    project = destination / "project"
    control = destination / "control"
    project.mkdir()
    control.mkdir()
    for name, data in files.items():
        write_new(project / name, data)
    write_new(control / "task.txt", task)
    write_new(control / "verify.json", spec)
    descriptor = {
        "schema": 1, "kind": "host-setup-inputs-not-an-approved-plan",
        "slot": slot, "workspace": str(project),
        "requirementFile": "control/task.txt", "artifactPaths": sorted(files),
        "verification": {
            "specFile": "control/verify.json",
            "launcher": str(root / PINS["launcher"][0]),
            "launcherSha256": PINS["launcher"][1],
            "checker": str(root / PINS["checker"][0]),
            "checkerSha256": PINS["checker"][1],
            "nodeExecutable": None, "nodeSha256": None,
            "registrationRequired": True,
        },
        "decomposition": workload["decomposition"] if slot["mode"] == "team"
        else [{"id": "complete", "files": sorted(files), "dependsOn": []}],
        "configuration": None, "approvedPlanDigest": None,
        "authorization": None, "readyToDispatch": False,
        "requiredBeforeExecution": [
            "Pin equal worker model/tools/parameters across both modes",
            "Freeze role configuration, per-slot limits and total budget",
            "Bind a true single-executor route without management model calls",
            "Register pinned independent checker and all required branch checks",
            "Initialize Git/worktree and configure via existing Host",
            "Obtain concrete real-run authorization and approve current plan",
            "Bind original ledger, terminal checks, usage and wall-clock evidence",
        ],
    }
    write_new(control / "host-inputs.json", json_bytes(descriptor))
    index = {p.relative_to(destination).as_posix(): sha(p.read_bytes())
             for p in destination.rglob("*") if p.is_file()}
    expected = {f"project/{name}" for name in files} | {
        "control/task.txt", "control/verify.json", "control/host-inputs.json"}
    require(set(index) == expected, "Unexpected staged content")
    ready = {
        "schema": 1, "status": "prepared-not-authorized", "slotId": slot_id,
        "mode": slot["mode"], "workload": slot["task"], "files": index,
        "sourcePins": {name: {"path": rel, "sha256": digest}
                       for name, (rel, digest) in PINS.items()},
        "preparerSha256": sha(Path(__file__).read_bytes()),
        "referenceFilesCopied": 0, "realModelCalls": 0, "dispatches": 0,
        "readyToDispatch": False, "approved": False,
        "workspaceIsSecuritySandbox": False, "releaseAccepted": False,
    }
    write_new(destination / "ready.json", json_bytes(ready))
    return ready


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--slot", required=True)
    parser.add_argument("--destination", required=True, type=Path)
    args = parser.parse_args()
    ready = prepare(args.slot, args.destination)
    print(json.dumps({k: ready[k] for k in
                      ("status", "slotId", "realModelCalls", "readyToDispatch")}))
