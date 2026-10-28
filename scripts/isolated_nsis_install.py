"""One-shot internal NSIS installation driver. No cleanup, retry, or product launch.

The NSIS /D suffix deliberately does not use subprocess.list2cmdline: NSIS
parses the final raw suffix, unlike programs using the Windows CRT argv rules.
Use --dry-run for all checks without launching the installer. A run directory
is consumed even by a failed/dry run; use a new directory for a reviewed retry.
"""
from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
from datetime import datetime, timezone
import uuid

ROOT = Path(__file__).resolve().parent.parent
PRODUCT_KEY = r"Software\ceruleanlonging\CeruleanLonging"
UNINSTALL_KEY = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CeruleanLonging"
PAYLOADS = ("CeruleanLonging.exe", "cerulean-runtime.exe")
RESOURCE_LIMITS = {"SQLITE-NOTICES.txt": 1024 * 1024,
                   "THIRD-PARTY-NOTICES.txt": 8 * 1024 * 1024,
                   "RUST-LIBRARY-COPYRIGHT.html": 4 * 1024 * 1024,
                   "THIRD-PARTY-SOURCES.zip": 16 * 1024 * 1024,
                   "THIRD-PARTY-MANIFEST.json": 1024 * 1024}


def now():
    return datetime.now(timezone.utc).isoformat()


def safe_text(value):
    value = str(value)
    if not value or '"' in value or any(ord(c) < 32 or ord(c) == 127 for c in value):
        raise ValueError("Empty, quoted, or control-character path refused")
    return value


def raw_nsis_command(executable, destination):
    exe, dest = safe_text(executable), safe_text(destination)
    if not re.match(r"^[A-Za-z]:\\", exe) or not re.match(r"^[A-Za-z]:\\", dest):
        raise ValueError("NSIS requires normalized absolute local drive paths")
    # The executable alone is quoted. Nothing may follow the raw /D suffix.
    return '"' + exe + '" /S /NS /D=' + dest


def no_reparse(path):
    path = Path(path)
    for part in (path, *path.parents):
        try:
            info = part.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise ValueError("Reparse/symlink path refused: " + str(part))


def owned_run(reports, requested):
    reports = Path(os.path.abspath(safe_text(reports)))
    run = Path(os.path.abspath(safe_text(requested)))
    no_reparse(reports)
    no_reparse(run)
    # One new direct child prevents accidentally selecting a prior transaction
    # or an existing install tree as the evidence/installation destination.
    if os.path.normcase(str(run.parent)) != os.path.normcase(str(reports)):
        raise ValueError("Run must be a new direct child of the project reports directory")
    if not run.name.startswith("isolated-nsis-") or run.exists():
        raise ValueError("Run must be new and named isolated-nsis-...")
    if len(str(run / "installed app" / "cerulean-runtime.exe")) >= 260:
        raise ValueError("Owned installation exceeds the conservative NSIS path bound")
    return run


def digest(path):
    h = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def save_new(path, document):
    # Never replace a prior request, observation, or failure report.
    with Path(path).open("x", encoding="utf-8", newline="\n") as stream:
        json.dump(document, stream, indent=2, ensure_ascii=False)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())


def registry_snapshot():
    import winreg
    entries = []
    for hive_name, hive in (("HKCU", winreg.HKEY_CURRENT_USER), ("HKLM", winreg.HKEY_LOCAL_MACHINE)):
        for view_name, view in (("64", winreg.KEY_WOW64_64KEY), ("32", winreg.KEY_WOW64_32KEY)):
            for key_path in (PRODUCT_KEY, UNINSTALL_KEY):
                entry = {"hive": hive_name, "view": view_name, "key": key_path, "present": False}
                try:
                    with winreg.OpenKey(hive, key_path, 0, winreg.KEY_READ | view) as key:
                        entry["present"] = True
                        entry["values"] = {}
                        for name in ("", "DisplayVersion", "InstallLocation", "UninstallString"):
                            try:
                                value, kind = winreg.QueryValueEx(key, name)
                                entry["values"][name] = {"value": value, "kind": kind}
                            except FileNotFoundError:
                                pass
                except FileNotFoundError:
                    pass
                # Access denied is not evidence that a registry key is absent.
                entries.append(entry)
    return entries


def require_no_registration(entries):
    if any(entry["present"] for entry in entries):
        raise ValueError("Existing product registration detected; installation refused")


def snapshot_tree(path):
    path = Path(path)
    no_reparse(path)
    if not path.exists():
        return {"path": str(path), "exists": False, "entries": []}
    if not path.is_dir():
        raise ValueError("Expected directory: " + str(path))
    entries, total = [], 0
    for folder, dirs, files in os.walk(path, followlinks=False):
        for name in sorted(dirs + files):
            entry = Path(folder) / name
            no_reparse(entry)
            info = entry.stat()
            row = {"path": entry.relative_to(path).as_posix(), "directory": entry.is_dir()}
            if not row["directory"]:
                total += info.st_size
                if total > 2 * 1024**3:
                    raise ValueError("Snapshot bound exceeded; no installation permitted")
                row.update(bytes=info.st_size, sha256=digest(entry))
            entries.append(row)
            if len(entries) > 100000:
                raise ValueError("Snapshot bound exceeded; no installation permitted")
    return {"path": str(path), "exists": True, "entries": sorted(entries, key=lambda x: x["path"])}


def package_resources(manifest):
    version = manifest.get("version", "")
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("Unexpected package version")
    parsed = tuple(map(int, version.split(".")))
    resources = manifest.get("resources", [])
    if not isinstance(resources, list) or any(not isinstance(item, dict) for item in resources):
        raise ValueError("Invalid notice resource declarations")
    expected = set()
    if parsed >= (0, 1, 22):
        expected = set(RESOURCE_LIMITS)
    elif parsed >= (0, 1, 21) or resources:
        expected = {"SQLITE-NOTICES.txt"}
    names = [item.get("path") for item in resources]
    if (any(not isinstance(name, str) for name in names)
            or len(resources) != len(expected) or set(names) != expected):
        raise ValueError("Required notice resource missing, duplicate or unexpected")
    for item in resources:
        size, checksum = item.get("bytes"), item.get("sha256")
        if (type(size) is not int or not 0 < size <= RESOURCE_LIMITS[item["path"]]
                or not isinstance(checksum, str) or not re.fullmatch(r"[a-f0-9]{64}", checksum)):
            raise ValueError("Notice size or digest declaration invalid")
    return resources


def check_resources(directory, resources):
    for item in resources:
        path = Path(directory) / item["path"]
        no_reparse(path)
        if (not path.is_file() or path.stat().st_size != item["bytes"]
                or digest(path) != item["sha256"]):
            raise ValueError("Notice resource missing or mismatched: " + item["path"])


def inspect_package(package, manifest_sha256):
    package = Path(os.path.abspath(safe_text(package)))
    no_reparse(package)
    if not re.fullmatch(r"[0-9a-fA-F]{64}", manifest_sha256):
        raise ValueError("A fixed manifest SHA256 is required")
    manifest_path = package / "manifest.json"
    no_reparse(manifest_path)
    with manifest_path.open("rb") as stream:
        raw = stream.read(1024 * 1024 + 1)
    if len(raw) > 1024 * 1024:
        raise ValueError("Package manifest exceeds size bound")
    if hashlib.sha256(raw).hexdigest() != manifest_sha256.lower():
        raise ValueError("Package manifest hash mismatch")
    manifest = json.loads(raw.decode("utf-8-sig"))
    if manifest.get("product") != "CeruleanLonging" or manifest.get("identifier") != "local.ceruleanlonging.workbench":
        raise ValueError("Unexpected package identity")
    resources = package_resources(manifest)
    artifacts = {item["path"]: item for item in manifest["artifacts"]}
    if len(artifacts) != len(manifest["artifacts"]):
        raise ValueError("Duplicate package artifact names")
    for name in ("setup.exe", *PAYLOADS):
        artifact = artifacts[name]
        path = package / name
        no_reparse(path)
        if path.stat().st_size != artifact["bytes"] or digest(path) != artifact["sha256"]:
            raise ValueError("Package artifact mismatch: " + name)
    check_resources(package, resources)
    return package, manifest


def validate_installed(install, manifest, registration):
    install = Path(install)
    no_reparse(install)
    resources = package_resources(manifest)
    artifacts = {item["path"]: item for item in manifest["artifacts"]}
    for name in PAYLOADS:
        path = install / name
        no_reparse(path)
        if path.stat().st_size != artifacts[name]["bytes"] or digest(path) != artifacts[name]["sha256"]:
            raise ValueError("Installed payload mismatch: " + name)
    check_resources(install, resources)
    no_reparse(install / "uninstall.exe")
    if not (install / "uninstall.exe").is_file():
        raise ValueError("Uninstaller missing from owned destination")
    found = set()
    for entry in registration:
        if not entry["present"]:
            continue
        if entry["hive"] != "HKCU":
            raise ValueError("Unexpected machine installation registration")
        values = {name: item["value"] for name, item in entry["values"].items()}
        if entry["key"] == PRODUCT_KEY:
            if values.get("") != str(install):
                raise ValueError("Product registration points outside the owned installation")
        else:
            expected = {"DisplayVersion": manifest["version"], "InstallLocation": '"' + str(install) + '"',
                        "UninstallString": '"' + str(install / "uninstall.exe") + '"'}
            if any(values.get(name) != value for name, value in expected.items()):
                raise ValueError("Installed version/location/uninstall registration mismatch")
        found.add(entry["key"])
    if found != {PRODUCT_KEY, UNINSTALL_KEY}:
        raise ValueError("Both owned product and uninstall registrations are required")


def known_folder(csidl):
    buffer = ctypes.create_unicode_buffer(32768)
    if ctypes.windll.shell32.SHGetFolderPathW(None, csidl, None, 0, buffer) != 0:
        raise OSError("Unable to resolve Windows known folder")
    return Path(buffer.value)


def observe_installer(child, run, timeout, result):
    """Keep ownership of a started child even when observation receipts fail.

    A receipt failure is retained as a failure after observing the original
    process. It never authorizes another launch or a successful installation.
    """
    errors = []
    result["observationErrors"] = errors

    def preserve(label, action):
        try:
            action()
        except BaseException as error:
            # Even an interrupt while publishing evidence must not abandon the
            # child and its PIPEs. Report the interruption after observation.
            errors.append({"stage": label, "type": type(error).__name__})

    preserve("original-process.json", lambda: save_new(run / "original-process.json", {
        "at": now(), "pid": child.pid,
        "handleValueForDiagnosticsOnly": int(child._handle),
        "handleNotTransferable": True, "waitUsesOriginalPopenHandle": True}))
    timed_out = False
    remaining_timeout = timeout
    while True:
        try:
            stdout, stderr = child.communicate(timeout=remaining_timeout)
            break
        except subprocess.TimeoutExpired:
            timed_out = True
            remaining_timeout = None
            preserve("timeout-observation.json", lambda: save_new(run / "timeout-observation.json", {
                "at": now(), "pid": child.pid,
                "status": "observation-only-waiting-on-original-handle", "retryAllowed": False,
                "terminationAttempted": False}))
            preserve("timeout-notification", lambda: print(json.dumps({
                "run": str(run), "status": "still-observing-original-process",
                "pid": child.pid}), flush=True))
        except KeyboardInterrupt as error:
            errors.append({"stage": "communicate", "type": type(error).__name__})
            remaining_timeout = None
            # Continue draining the same Popen, not a replacement or PID lookup.

    result.update(installerExitCode=child.returncode, originalProcessObserved=True,
                  stdoutEOF=True, stderrEOF=True, initialTimeout=timed_out)

    def save_log(name, data):
        with (run / name).open("xb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())

    preserve("installer.stdout", lambda: save_log("installer.stdout", stdout))
    preserve("installer.stderr", lambda: save_log("installer.stderr", stderr))
    preserve("finished.json", lambda: save_new(run / "finished.json", {
        "at": now(), "pid": child.pid, "exit": child.returncode,
        "waitedOriginalHandle": True, "stdoutEOF": True, "stderrEOF": True,
        "initialTimeout": timed_out, "observationErrors": list(errors)}))
    if errors:
        result["status"] = "observation-evidence-failed"
        raise RuntimeError("Original installer observed; evidence failed, preserve the original run")
    return timed_out


def execute(args):
    if os.name != "nt":
        raise RuntimeError("Windows only")
    run = owned_run(ROOT / "reports", args.run_dir or ROOT / "reports" / ("isolated-nsis-" + uuid.uuid4().hex))
    install = run / "installed app"
    package, manifest = inspect_package(args.package, args.manifest_sha256)
    command = raw_nsis_command(package / "setup.exe", install)
    run.mkdir(exist_ok=False)
    result = {"run": str(run), "at": now(), "status": "preflight-failed", "installerLaunches": 0, "launchAttempts": 0,
              "automaticRetry": False, "cleanupAttempted": False, "productLaunched": False,
              "scope": "isolated current-account installation only, not clean-machine or full P6"}
    try:
        request = {"at": now(), "commandLine": command, "executable": str(package / "setup.exe"),
                   "installedDirectory": str(install), "manifestSha256": args.manifest_sha256.lower(),
                   "version": manifest["version"], "dryRun": args.dry_run, "timeoutSeconds": args.timeout,
                   "serialization": "raw command string; shell=False; /D last and unquoted"}
        request["resources"] = package_resources(manifest)
        save_new(run / "request.json", request)
        registration = registry_snapshot()
        save_new(run / "registry-before.json", registration)
        require_no_registration(registration)
        defaults = {"defaultInstall": known_folder(0x1C) / "CeruleanLonging",
                    "standardData": known_folder(0x1A) / "local.ceruleanlonging.workbench" / "workspace"}
        before = {name: snapshot_tree(path) for name, path in defaults.items()}
        save_new(run / "directories-before.json", before)
        if before["defaultInstall"]["exists"]:
            raise ValueError("Unregistered default installation exists; ownership review required")
        if args.dry_run:
            result["status"] = "dry-run-ready-no-installation"
            return result
        # Re-read immediately before the one permitted launch. A global guarantee
        # against independent external installers requires a separate account/VM.
        require_no_registration(registry_snapshot())
        no_reparse(run)
        if install.exists():
            raise ValueError("Installation destination appeared after preflight")
        inspect_package(package, args.manifest_sha256)
        save_new(run / "startup-intent.json", {"at": now(), "commandLine": command, "replayAllowed": False})
        result["status"] = "startup-unknown"
        result["launchAttempts"] = 1
        result["installerLaunches"] = None
        # Pass the raw command string: a list would quote the entire /D argument.
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = subprocess.SW_HIDE
        child = subprocess.Popen(command, executable=str(package / "setup.exe"), cwd=str(run),
                                 shell=False, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 startupinfo=startup, creationflags=subprocess.CREATE_NO_WINDOW)
        result["installerLaunches"] = 1
        timed_out = observe_installer(child, run, args.timeout, result)
        result["status"] = "post-install-verification-failed"
        after_registration = registry_snapshot()
        save_new(run / "registry-after.json", after_registration)
        after = {name: snapshot_tree(path) for name, path in defaults.items()}
        save_new(run / "directories-after.json", after)
        if child.returncode != 0:
            raise ValueError("Installer exited with failure: " + str(child.returncode))
        if after != before:
            raise ValueError("Default installation or standard data changed; preserve all evidence")
        validate_installed(install, manifest, after_registration)
        save_new(run / "installed-inventory.json", snapshot_tree(install))
        result.update(status="passed" if not timed_out else "verified-after-timeout-requires-review",
                      installedDirectory=str(install), version=manifest["version"], payloadsMatch=True,
                      resourcesMatch=True, verifiedResourceCount=len(package_resources(manifest)),
                      defaultDirectoriesUnchanged=True, registrationMatches=True)
        return result
    except Exception as error:
        result.update(error=repr(error), evidencePreserved=True)
        return result
    finally:
        result["finishedAt"] = now()
        save_new(run / "result.json", result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", required=True)
    parser.add_argument("--manifest-sha256", required=True)
    parser.add_argument("--run-dir")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--timeout", type=int, default=120)
    args = parser.parse_args()
    if not 1 <= args.timeout <= 600:
        parser.error("--timeout must be between 1 and 600 seconds")
    result = execute(args)
    print(json.dumps(result, ensure_ascii=False))
    return 0 if result["status"] in ("passed", "dry-run-ready-no-installation") else 2


if __name__ == "__main__":
    sys.exit(main())
