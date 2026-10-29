"""Immutable experiment materials, not an executor or an authorization authority."""
from datetime import datetime
import argparse
import importlib.util
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


staging = module("efficiency_staging", ROOT / "scripts/prepare-efficiency-workspace.py")
usage = module("efficiency_usage", ROOT / "scripts/efficiency-usage.py")
require = staging.require
sha = staging.sha
KINDS = {"authorization-note", "runtime-binding", "start-observation", "stop-observation", "core-export"}


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode()


def read(path):
    return json.loads(staging.no_links(path).read_bytes())


def inventory(directory):
    """Reject links and unexpected directories, including empty ones."""
    files, folders = set(), set()
    for path in directory.rglob("*"):
        staging.no_links(path)
        name = path.relative_to(directory).as_posix()
        if path.is_dir():
            folders.add(name)
        else:
            require(path.is_file() and path.stat().st_nlink == 1, "Non-regular or hard-linked input")
            files.add(name)
    return files, folders


def expected_folders(files):
    return {parent.as_posix() for name in files for parent in Path(name).parents
            if parent != Path(".")}


def pinned_slot(slot_id):
    _, _, schedule = staging.load_suite()
    matches = [s for s in schedule["slots"] if s["id"] == slot_id]
    require(len(matches) == 1, "Unknown staged slot")
    return matches[0]


def seed_spec(slot):
    return {**{f"project/{name}": digest for name, digest in slot["seed"].items()},
            "control/task.txt": slot["taskSha256"], "control/verify.json": slot["checkerSha256"]}


def write(path, data):
    with path.open("xb") as stream:
        stream.write(data)
        stream.flush()
        import os
        os.fsync(stream.fileno())


def save(path, value):
    write(path, (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode())


def config_check(config, mode):
    require(isinstance(config, dict) and
            set(config) == {"schema", "executionKind", "mode", "worker", "management", "limits"},
            "Unexpected configuration fields; credentials must not be included")
    require(type(config["schema"]) is int and config["schema"] == 1 and config["mode"] == mode
            and config["executionKind"] in ("synthetic", "real-attempt"), "Configuration classification")
    worker = config["worker"]
    require(isinstance(worker, dict) and set(worker) - {"writeScope"} == {"target", "model", "fileTools", "maxRounds", "maxToolCalls",
                           "maxOutputTokens", "maxTotalTokens", "requestTimeoutSeconds"}, "Worker configuration shape")
    require("writeScope" not in worker or worker["writeScope"] == "declared-artifacts",
            "Unsupported worker write scope; omit for legacy workspace scope")
    require(isinstance(worker["target"], dict), "Invalid worker target")
    usage.target_provider(worker["target"])
    # Provider endpoints carry only their documented identity, never arbitrary URLs or keys.
    target = worker["target"]
    if target["kind"] == "provider":
        endpoint = target["endpoint"]
        require(set(target) == {"kind", "endpoint"}, "Unexpected target fields")
        require(set(endpoint) == ({"kind", "workspaceId"} if endpoint["kind"] == "qwenBeijing" else {"kind"}),
                "Unexpected endpoint fields")
        if endpoint["kind"] == "qwenBeijing":
            require(endpoint["workspaceId"] is None or
                    isinstance(endpoint["workspaceId"], str) and
                    re.fullmatch(r"[A-Za-z0-9_-]{1,100}", endpoint["workspaceId"]),
                    "Invalid workspace identifier")
    require(isinstance(worker["model"], str) and re.fullmatch(r"[A-Za-z0-9_.:/-]{1,200}", worker["model"]),
            "Invalid worker model")
    require(worker["fileTools"] == ["read-files", "write-files"], "Freeze the same file-only worker tools")
    for field, maximum in (("maxRounds", 128), ("maxToolCalls", 1000), ("maxOutputTokens", 16384),
                           ("maxTotalTokens", 10000000), ("requestTimeoutSeconds", 600)):
        require(type(worker[field]) is int and 1 <= worker[field] <= maximum, "Invalid worker limit: " + field)
    managers = config["management"]
    require(isinstance(managers, list), "Management configuration must be a list")
    if mode == "single":
        require(not managers, "Single baseline cannot declare management model calls")
    else:
        require(len(managers) == 2 and all(isinstance(m, dict) for m in managers)
                and {m.get("role") for m in managers} == {"chief", "deputy"},
                "Team requires chief and deputy configuration")
        for manager in managers:
            require(set(manager) == {"role", "source", "model"}, "Unexpected management fields")
            require(manager["source"] in ("openai-api", "deepseek-api", "codex-session", "a6api-api"),
                    "Unsupported management source")
            require(manager["role"] != "deputy" or manager["source"] != "codex-session",
                    "Codex session source is chief-only")
            require(isinstance(manager["model"], str)
                    and re.fullmatch(r"[A-Za-z0-9_.:/-]{1,200}", manager["model"]), "Invalid management model")
    limits = config["limits"]
    require(isinstance(limits, dict) and
            set(limits) == {"taskSeconds", "workerRequests", "managementRequests", "failurePolicy"},
            "Unexpected limit fields")
    require(limits["failurePolicy"] == "stop-no-retry", "Explicit stop policy required")
    for field in ("taskSeconds", "workerRequests", "managementRequests"):
        require(type(limits[field]) is int and 0 <= limits[field] <= 10000, "Invalid experiment limit")
    require(limits["taskSeconds"] > 0 and limits["workerRequests"] > 0, "Positive task limits required")
    require((limits["managementRequests"] == 0) == (mode == "single"), "Management limit differs from mode")


def freeze(stage, configuration, destination):
    stage = staging.no_links(stage)
    ready = read(stage / "ready.json")
    slot = pinned_slot(ready["slotId"])
    require(ready["mode"] == slot["mode"] and ready["workload"] == slot["task"],
            "Mismatched staged slot")
    require(ready["status"] == "prepared-not-authorized" and ready["readyToDispatch"] is False
            and ready["approved"] is False,
            "Expected an unstarted prepared workspace")
    config = read(configuration)
    config_check(config, slot["mode"])
    expected = seed_spec(slot)
    files = {}
    for name, digest in ready["files"].items():
        data = staging.checked_read(stage, name, digest)
        files[name] = data
    require(set(files) == set(expected) | {"control/host-inputs.json"}, "Unexpected staged files")
    for name, digest in expected.items():
        require(sha(files[name]) == digest, "Staged content differs from original workload")
    require(set(ready["sourcePins"]) == set(staging.PINS), "Missing or unexpected source pins")
    for name, pin in ready["sourcePins"].items():
        require((pin["path"], pin["sha256"]) == staging.PINS[name], "Source pin changed")
    actual, folders = inventory(stage)
    require(actual == set(files) | {"ready.json"} and folders == expected_folders(actual),
            "Unexpected entry in initial workspace")
    destination = staging.no_links(destination)
    require(not destination.exists() and destination.parent.is_dir(), "Bundle destination must be new")
    require(not destination.is_relative_to(stage), "Bundle must stay outside worker workspace")
    destination.mkdir()
    (destination / "inputs").mkdir()
    (destination / "attachments").mkdir()
    # Only validated seed/spec bytes are copied. The original project and ready
    # files remain outside this bundle and are not changed.
    archived = {}
    for name, data in files.items():
        if name == "control/host-inputs.json":
            continue
        dest = destination / "inputs" / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        write(dest, data)
        archived[dest.relative_to(destination).as_posix()] = sha(data)
    config_bytes = canonical(config)
    write(destination / "configuration.json", config_bytes)
    manifest = dict(schema=1, kind="experiment-materials-not-an-approved-run",
        slot=slot, preparedAt=datetime.now().astimezone().isoformat(),
        originalWorkspace=str(stage / "project"), proposedProjectId="eff-" + slot["id"],
        preparedReadySha256=sha((stage / "ready.json").read_bytes()),
        configurationSha256=sha(config_bytes), workerConfigurationSha256=sha(canonical(config["worker"])),
        inputFiles=archived, declaredExecutionKind=config["executionKind"],
        actualExecutionKind=None, authorized=False, experimentRecordReady=False, releaseAccepted=False)
    save(destination / "manifest.json", manifest)
    return manifest


def inspect(directory, expected_manifest_sha):
    directory = staging.no_links(directory)
    require(isinstance(expected_manifest_sha, str) and
            re.fullmatch(r"[a-f0-9]{64}", expected_manifest_sha), "Pinned manifest digest required")
    require(sha((directory / "manifest.json").read_bytes()) == expected_manifest_sha,
            "Manifest changed from caller-selected digest")
    manifest = read(directory / "manifest.json")
    require(manifest["schema"] == 1 and manifest["kind"] == "experiment-materials-not-an-approved-run"
            and manifest["authorized"] is False and manifest["experimentRecordReady"] is False
            and manifest["releaseAccepted"] is False and manifest["actualExecutionKind"] is None,
            "Unexpected manifest")
    slot = pinned_slot(manifest["slot"]["id"])
    require(manifest["slot"] == slot and manifest["proposedProjectId"] == "eff-" + slot["id"],
            "Manifest slot differs from pinned schedule")
    require(manifest["inputFiles"] == {"inputs/" + k: v for k, v in seed_spec(slot).items()},
            "Manifest input set differs from pinned workload")
    config = read(directory / "configuration.json")
    require(sha((directory / "configuration.json").read_bytes()) == manifest["configurationSha256"],
            "Frozen configuration changed")
    config_check(config, manifest["slot"]["mode"])
    require(config["executionKind"] == manifest["declaredExecutionKind"], "Execution declaration changed")
    require(sha(canonical(config["worker"])) == manifest["workerConfigurationSha256"], "Worker config changed")
    for name, digest in manifest["inputFiles"].items():
        staging.checked_read(directory, name, digest)
    attachments = {}
    pending = []
    allowed = set(manifest["inputFiles"]) | {"manifest.json", "configuration.json"}
    for kind in sorted(KINDS):
        intent = directory / "attachments" / (kind + ".intent.json")
        receipt = directory / "attachments" / (kind + ".receipt.json")
        blob = directory / "attachments" / (kind + ".json")
        for path in (intent, receipt, blob):
            if path.exists():
                allowed.add(path.relative_to(directory).as_posix())
        if receipt.exists():
            recorded = read(receipt)
            require(intent.exists() and read(intent) == recorded, "Attachment receipt lacks its intent")
            require(recorded["slotId"] == manifest["slot"]["id"] and recorded["kind"] == kind,
                    "Attachment scope mismatch")
            staging.checked_read(directory, blob.relative_to(directory).as_posix(), recorded["sha256"])
            attachments[kind] = recorded
        elif intent.exists() or blob.exists():
            pending.append(kind)
    require(not pending, "Incomplete attachment preserved; no automatic retry: " + ",".join(pending))
    actual, folders = inventory(directory)
    require(actual == allowed and folders == expected_folders(allowed) | {"attachments"},
            "Unexpected entry in experiment bundle")
    for kind in attachments:
        attachment_check(directory, manifest, attachments, kind,
                         read(directory / "attachments" / (kind + ".json")))
    return manifest, attachments


def attachment_check(directory, manifest, attachments, kind, value):
    require(isinstance(value, dict), "Attachment must be a JSON object")
    if kind == "runtime-binding":
        require(set(value) == {"projectId", "revisionDigest"} and value["projectId"] == manifest["proposedProjectId"]
                and re.fullmatch(r"[a-f0-9]{64}", value["revisionDigest"]), "Runtime binding mismatch")
    elif kind == "core-export":
        require("runtime-binding" in attachments, "Record runtime identity before its export")
        binding = read(directory / "attachments/runtime-binding.json")
        require(value["projectId"] == binding["projectId"] and value["revisionDigest"] == binding["revisionDigest"],
                "Export belongs to another run")
        usage.analyze(value)  # Includes cross-channel identity and unknown-usage checks.
    else:
        require(value.get("slotId") == manifest["slot"]["id"]
                and value.get("configurationSha256") == manifest["configurationSha256"],
                "Observation belongs to another slot/configuration")


def attach(directory, kind, source, expected_sha, expected_manifest_sha):
    directory = staging.no_links(directory)
    require(kind in KINDS, "Unsupported attachment kind")
    require(re.fullmatch(r"[a-f0-9]{64}", expected_sha), "Expected attachment digest required")
    manifest, attachments = inspect(directory, expected_manifest_sha)
    require(kind not in attachments, "Attachment already recorded; do not overwrite")
    raw = staging.no_links(source).read_bytes()
    require(sha(raw) == expected_sha, "Attachment source changed")
    value = json.loads(raw)
    attachment_check(directory, manifest, attachments, kind, value)
    receipt = dict(kind=kind, slotId=manifest["slot"]["id"], sha256=expected_sha,
                   sourceContentAuthenticated=False)
    folder = directory / "attachments"
    # The exclusive intent also arbitrates concurrent attachment attempts.
    save(folder / (kind + ".intent.json"), receipt)
    write(folder / (kind + ".json"), raw)
    save(folder / (kind + ".receipt.json"), receipt)
    return receipt


def review(directory, expected_manifest_sha):
    manifest, attachments = inspect(directory, expected_manifest_sha)
    return dict(slotId=manifest["slot"]["id"], frozenInputsVerified=True,
        attachmentKinds=sorted(attachments), missingAttachmentKinds=sorted(KINDS - attachments.keys()),
        evidenceContentAuthenticated=False, authorized=False, actualExecutionKind=None,
        wholeRunUsageComplete=False, experimentRecordReady=False, releaseAccepted=False,
        remainingProof=["Runtime plan/task/seed/worktree binding",
                        "Authentic authorization and original pre-run source inventory",
                        "Runner-produced full monotonic timing and terminal classification",
                        "Real single-executor route and experiment/model provenance"])


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    create = sub.add_parser("freeze")
    create.add_argument("--stage", type=Path, required=True)
    create.add_argument("--configuration", type=Path, required=True)
    create.add_argument("--directory", type=Path, required=True)
    attachment = sub.add_parser("attach")
    attachment.add_argument("--directory", type=Path, required=True)
    attachment.add_argument("--kind", choices=sorted(KINDS), required=True)
    attachment.add_argument("--file", type=Path, required=True)
    attachment.add_argument("--expected-sha256", required=True)
    attachment.add_argument("--expected-manifest-sha256", required=True)
    check = sub.add_parser("review")
    check.add_argument("--directory", type=Path, required=True)
    check.add_argument("--expected-manifest-sha256", required=True)
    args = parser.parse_args()
    if args.command == "freeze":
        freeze(args.stage, args.configuration, args.directory)
        digest = sha((args.directory / "manifest.json").read_bytes())
    elif args.command == "attach":
        digest = args.expected_manifest_sha256
        attach(args.directory, args.kind, args.file, args.expected_sha256, digest)
    else:
        digest = args.expected_manifest_sha256
    print(json.dumps(dict(manifestSha256=digest, **review(args.directory, digest)), ensure_ascii=False))
