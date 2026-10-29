"""Resume only the reviewed junction-format failure; no model calls or deletes."""
from prepare_qoder_migration_226 import (
    REPORT, MAPPINGS, require_closed, manifest, save, subprocess, json, datetime
)

require_closed()
receipts = []
for source, target in MAPPINGS[:2]:
    receipt = json.loads((REPORT / f"{target.name}-verified.json").read_text(encoding="utf-8"))
    assert receipt["source"] == str(source) and receipt["destination"] == str(target)
    assert receipt["sha256Verified"] and receipt["sourceUnchanged"]
    receipts.append(receipt)

for source, target in MAPPINGS[2:]:
    require_closed()
    snapshot = REPORT / f"{target.name}-source.json"
    if target.name == "user-profile":
        print("Rechecking corrected user-profile junctions and contents", flush=True)
        before = json.loads(snapshot.read_text(encoding="utf-8"))
        exit_code = None  # Previous copy succeeded but exit was not persisted.
    else:
        before = manifest(source)
        save(snapshot, before)
        with (REPORT / f"{target.name}-robocopy-data.log").open("xb") as log:
            copied = subprocess.run(
                ["robocopy.exe", str(source), str(target), "/E", "/XJ", "/COPY:DAT",
                 "/DCOPY:DAT", "/R:0", "/W:0", "/NFL", "/NDL", "/NP"],
                stdout=log, stderr=subprocess.STDOUT,
            )
        exit_code = copied.returncode
        assert exit_code < 8
    assert before == manifest(target) == manifest(source), "Full comparison failed; no switch"
    receipt = {
        "source": str(source), "destination": str(target),
        "files": len(before["files"]),
        "bytes": sum(f["bytes"] for f in before["files"].values()),
        "junctions": len(before["junctions"]),
        "sha256Verified": True, "sourceUnchanged": True,
        "robocopyExit": exit_code,
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
print("All copies verified; original source paths still unchanged", flush=True)
