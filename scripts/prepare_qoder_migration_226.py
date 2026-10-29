"""One-shot copy and content verification; never deletes or switches source paths."""
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
from datetime import datetime

REPORT = Path(r"D:\test\CeruleanLonging\reports\qoder-migration-226")
DESTINATION = Path(r"D:\Qoder")
MAPPINGS = [
    (Path(r"E:\千问\Qoder"), DESTINATION / "app"),
    (Path(r"E:\qoder-data"), DESTINATION / "data"),
    (Path(r"C:\Users\姚\.qoder"), DESTINATION / "user-profile"),
    (Path(r"C:\Users\姚\AppData\Local\Qoder"), DESTINATION / "local"),
    (Path(r"C:\Users\姚\AppData\Roaming\Qoder"), DESTINATION / "roaming"),
]
PWSH = r"C:\Users\姚\.cache\codex-runtimes\codex-primary-runtime\dependencies\native\powershell\pwsh.exe"


def save(path, value):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)


def require_closed():
    command = "if (Get-Process -Name Qoder -ErrorAction SilentlyContinue) { exit 3 }"
    result = subprocess.run([PWSH, "-NoProfile", "-Command", command], capture_output=True)
    if result.returncode:
        raise RuntimeError("Qoder still running; no migration permitted")


def manifest(root):
    files, directories, junctions = {}, [], {}
    for current, dirs, names in os.walk(root, followlinks=False):
        for name in list(dirs):
            path = Path(current) / name
            rel = str(path.relative_to(root))
            if path.lstat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT:
                # Existing directory junctions only; do not traverse their data.
                junctions[rel] = os.readlink(path)
                dirs.remove(name)
            else:
                directories.append(rel)
        for name in names:
            path = Path(current) / name
            if path.lstat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT:
                raise RuntimeError("Unexpected file reparse point; needs review")
            digest = hashlib.sha256()
            with path.open("rb") as stream:
                for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                    digest.update(chunk)
            files[str(path.relative_to(root))] = {
                "bytes": path.stat().st_size, "sha256": digest.hexdigest()
            }
    return {"files": files, "directories": sorted(directories), "junctions": junctions}


def main():
    require_closed()
    resume = sys.argv[1:] == ["--resume-copy-after-acl-review"]
    if sys.argv[1:] and not resume:
        raise RuntimeError("Unknown invocation")
    if not resume and (DESTINATION.exists() or REPORT.exists()):
        raise RuntimeError("Owned migration destination/report already exists; inspect before resuming")
    if resume and (not (REPORT / "app-source.json").is_file() or (REPORT / "prepared.json").exists()):
        raise RuntimeError("Resume requires the reviewed incomplete first copy")
    for source, _ in MAPPINGS:
        if not source.is_dir() or source.lstat().st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT:
            raise RuntimeError("Source missing or already redirected")
    if not resume:
        REPORT.mkdir()
        DESTINATION.mkdir()
    # Destination root was explicitly restricted to current user/SYSTEM/Admins.
    # Robocopy /COPY:S requires rights unavailable here; never broaden access.
    acl_check = subprocess.run([PWSH, "-NoProfile", "-Command",
        "if (-not (Get-Acl -LiteralPath 'D:\\Qoder').AreAccessRulesProtected) { exit 4 }"],
        capture_output=True)
    if acl_check.returncode:
        raise RuntimeError("Private destination root ACL required")
    receipts = []
    for source, target in MAPPINGS:
        require_closed()
        print(f"Copying {target.name}", flush=True)
        before = manifest(source)
        snapshot = REPORT / f"{target.name}-source.json"
        if resume and snapshot.exists():
            if json.loads(snapshot.read_text(encoding="utf-8")) != before:
                raise RuntimeError("Original source changed since first preparation")
        else:
            save(snapshot, before)
        with (REPORT / f"{target.name}-robocopy-data.log").open("xb") as log:
            copied = subprocess.run(
                ["robocopy.exe", str(source), str(target), "/E", "/XJ", "/COPY:DAT",
                 "/DCOPY:DAT", "/R:0", "/W:0", "/NFL", "/NDL", "/NP"],
                stdout=log, stderr=subprocess.STDOUT,
            )
        if copied.returncode >= 8:
            raise RuntimeError("Copy failed; source untouched; inspect private log")
        for rel, link_target in before["junctions"].items():
            new_link = target / rel
            if new_link.exists():
                raise RuntimeError("Unexpected copied junction")
            # Native PowerShell filesystem API, with positional argument passing.
            helper = REPORT / "make-junction.ps1"
            if not helper.exists():
                helper.write_text(
                    "param([string]$LinkPath,[string]$TargetPath)\n"
                    "$ErrorActionPreference='Stop'\n"
                    "New-Item -ItemType Junction -Path $LinkPath -Target $TargetPath | Out-Null\n",
                    encoding="utf-8-sig",
                )
            result = subprocess.run(
                [PWSH, "-NoProfile", "-File", str(helper), str(new_link),
                 link_target[4:] if link_target.startswith("\\\\?\\") else link_target],
                capture_output=True,
            )
            if result.returncode:
                raise RuntimeError("Junction preservation failed; source untouched")
        print(f"Verifying {target.name}", flush=True)
        after = manifest(target)
        source_after = manifest(source)
        if before != after or before != source_after:
            raise RuntimeError("Content/source changed; no path switch permitted")
        receipt = {
            "source": str(source), "destination": str(target),
            "files": len(before["files"]),
            "bytes": sum(f["bytes"] for f in before["files"].values()),
            "junctions": len(before["junctions"]),
            "sha256Verified": True, "sourceUnchanged": True,
            "robocopyExit": copied.returncode,
            "permissions": "destination-inherits-private-root-current-user-system-admins",
        }
        save(REPORT / f"{target.name}-verified.json", receipt)
        receipts.append(receipt)
        print(f"Verified {target.name}: {receipt['files']} files", flush=True)
    require_closed()
    save(REPORT / "prepared.json", {
        "status": "copied-verified-not-switched",
        "createdAt": datetime.now().astimezone().isoformat(),
        "roots": receipts, "realProviderCalls": 0,
    })
    print("All copied contents verified; source paths remain untouched", flush=True)


if __name__ == "__main__":
    main()
