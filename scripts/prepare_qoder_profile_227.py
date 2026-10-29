"""Complete the explicitly authorized migration for actual Electron user data."""
import json
import sqlite3
import subprocess
from pathlib import Path
from datetime import datetime

from prepare_qoder_migration_226 import manifest, require_closed, save

REPORT = Path(r"D:\test\CeruleanLonging\reports\qoder-desktop-profile-227")
MAPPINGS = [
    (Path(r"C:\Users\姚\AppData\Roaming\com.qoder.app.stable"), Path(r"D:\Qoder\desktop-data")),
    (Path(r"C:\Users\姚\AppData\Local\qoder-updater"), Path(r"D:\Qoder\updater-cache")),
]


def database_check(root):
    db = root / "main.sqlite"
    # Only use immutable mode after a normal application shutdown and no WAL.
    assert not Path(str(db) + "-wal").exists(), "WAL present; do not ignore uncheckpointed data"
    with sqlite3.connect(db.as_uri() + "?mode=ro&immutable=1", uri=True) as connection:
        check = connection.execute("PRAGMA quick_check").fetchall()
        assert check == [("ok",)], "Database check failed; preserve original"
        count = connection.execute("SELECT COUNT(*) FROM byok_model_profiles").fetchone()[0]
    return {"quickCheck": "ok", "customModelProfiles": count}


def main():
    require_closed()
    assert not REPORT.exists()
    assert all(source.is_dir() and not target.exists() for source, target in MAPPINGS)
    REPORT.mkdir()
    records = []
    for source, target in MAPPINGS:
        print("Preparing " + target.name, flush=True)
        database_before = database_check(source) if target.name == "desktop-data" else None
        before = manifest(source)
        assert not before["junctions"], "Unexpected nested link; review before copying"
        save(REPORT / (target.name + "-source.json"), before)
        with (REPORT / (target.name + "-copy.log")).open("xb") as log:
            copied = subprocess.run(
                ["robocopy.exe", str(source), str(target), "/E", "/XJ", "/COPY:DAT",
                 "/DCOPY:DAT", "/R:0", "/W:0", "/NFL", "/NDL", "/NP"],
                stdout=log, stderr=subprocess.STDOUT,
            )
        assert copied.returncode < 8, "Copy failed; source untouched"
        print("Verifying " + target.name, flush=True)
        database_after = database_check(target) if database_before else None
        assert database_before == database_after
        assert before == manifest(target) == manifest(source), "Data changed; no switch"
        record = {
            "source": str(source), "destination": str(target),
            "files": len(before["files"]),
            "bytes": sum(item["bytes"] for item in before["files"].values()),
            "sha256Verified": True, "sourceUnchanged": True,
            "database": database_after, "robocopyExit": copied.returncode,
        }
        save(REPORT / (target.name + "-verified.json"), record)
        records.append(record)
        require_closed()
    save(REPORT / "prepared.json", {
        "status": "copied-verified-not-switched",
        "createdAt": datetime.now().astimezone().isoformat(),
        "roots": records, "newModelTasksSent": 0,
    })
    print("Desktop data and updater directory verified; originals untouched", flush=True)


if __name__ == "__main__":
    main()
