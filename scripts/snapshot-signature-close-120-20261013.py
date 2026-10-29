"""Read-only byte/certificate-set readback; no trust check or product execution."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
from datetime import datetime, timezone

root = Path(__file__).resolve().parent.parent
baseline_path = root / "reports/signature-offline-audit-120-20261013.json"
baseline = json.loads(baseline_path.read_text(encoding="utf-8"))
output = root / "reports/signature-close-consistency-120-20261013.json"
if output.exists():
    raise RuntimeError("Preserve existing close snapshot")


def observe(path):
    path = Path(path)
    path.resolve().relative_to(root.resolve())
    if not path.is_file():
        return {"exists": False}
    before = path.stat()
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for data in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(data)
    after = path.stat()
    return {"exists": True, "bytes": after.st_size, "sha256": digest.hexdigest(),
            "metadataStableDuringRead": (before.st_size, before.st_mtime_ns, before.st_ino) ==
                                        (after.st_size, after.st_mtime_ns, after.st_ino)}


items = [artifact["observed"] for package in baseline["packages"] for artifact in package["artifacts"]]
items += baseline["releaseArtifacts"]
artifacts = []
for item in items:
    current = observe(item["path"])
    artifacts.append({"path": item["path"], "expectedBytes": item["bytes"], "expectedSha256": item["sha256"],
                      "current": current, "unchanged": current.get("bytes") == item["bytes"] and
                      current.get("sha256") == item["sha256"] and current.get("metadataStableDuringRead", False)})
manifests = []
for package in baseline["packages"]:
    path = Path(package["path"]) / "manifest.json"
    current = observe(path)
    manifests.append({"path": str(path), "expectedSha256": package["manifestSha256"], "current": current,
                      "unchanged": current.get("sha256") == package["manifestSha256"] and
                      current.get("metadataStableDuringRead", False),
                      "priorLengthUnavailable": True})

discovered = set()
discovered_manifests = set()
for directory in (root / "reports").glob("p6-windows-package*"):
    manifest_path = directory / "manifest.json"
    if not directory.is_dir() or not manifest_path.is_file():
        continue
    discovered_manifests.add(str(manifest_path))
    manifest = json.loads(manifest_path.read_text(encoding="utf-8-sig"))
    for entry in manifest.get("artifacts", []):
        discovered.add(str(directory / entry["path"]))
for directory in ("target/release", "target/release/bundle/nsis", "apps/desktop/src-tauri/binaries"):
    discovered.update(str(path) for path in (root / directory).glob("*.exe") if path.is_file())
known = {item["path"] for item in artifacts}
new_paths = sorted(discovered - known)
new_manifests = sorted(discovered_manifests - {item["path"] for item in manifests})

# Read public certificate metadata only. No private-key property/export/signing,
# chain construction, trust changes, network or product execution is performed.
command = r"""
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$result=@()
foreach($location in @('CurrentUser','LocalMachine')){
 $store=[Security.Cryptography.X509Certificates.X509Store]::new('My',$location)
 $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadOnly -bor [Security.Cryptography.X509Certificates.OpenFlags]::OpenExistingOnly)
 try{foreach($cert in $store.Certificates){
  $eku=@($cert.Extensions|Where-Object {$_.Oid.Value -eq '2.5.29.37'}|ForEach-Object {$_.EnhancedKeyUsages}|ForEach-Object {$_.Value})
  if($eku -contains '1.3.6.1.5.5.7.3.3'){$result+=@{store=('Cert:\'+$location+'\My');subject=$cert.Subject;issuer=$cert.Issuer;thumbprint=$cert.Thumbprint;notBefore=$cert.NotBefore.ToUniversalTime().ToString('o');notAfter=$cert.NotAfter.ToUniversalTime().ToString('o');ekuOids=$eku}}
 }}finally{$store.Close()}
}
$trust=@()
$thumbs=@($result|ForEach-Object {$_.thumbprint})+@('6F064BFECC801ECF19407D443D0A1B7A84F0DC9E')
foreach($location in @('CurrentUser','LocalMachine')){
 foreach($name in @('Root','TrustedPublisher','TrustedPeople')){
  $store=[Security.Cryptography.X509Certificates.X509Store]::new($name,$location)
  $store.Open([Security.Cryptography.X509Certificates.OpenFlags]::ReadOnly -bor [Security.Cryptography.X509Certificates.OpenFlags]::OpenExistingOnly)
  try{foreach($cert in $store.Certificates){if($cert.Thumbprint -in $thumbs){$trust+=@{store=('Cert:\'+$location+'\'+$name);thumbprint=$cert.Thumbprint;subject=$cert.Subject;issuer=$cert.Issuer}}}}finally{$store.Close()}
 }
}
@{certificates=@($result);trustLocations=@($trust)}|ConvertTo-Json -Depth 8 -Compress
"""
powershell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
process = subprocess.Popen([str(powershell), "-NoProfile", "-NonInteractive", "-Command", command],
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                           creationflags=subprocess.CREATE_NO_WINDOW)
stdout, stderr = process.communicate(timeout=30)
if process.returncode != 0:
    raise RuntimeError("Certificate metadata readback failed: " + stderr.decode("utf-8", errors="replace"))
certificate_observation = json.loads(stdout.decode("utf-8-sig"))
certificates = certificate_observation["certificates"]
trust_locations = certificate_observation["trustLocations"]
prior_cert_set = {(item["store"], item["certificate"]["thumbprint"].upper()) for item in baseline["certificates"]}
current_cert_set = {(item["store"], item["thumbprint"].upper()) for item in certificates}
prior_trust_set = {(item["store"], item["certificate"]["thumbprint"].upper()) for item in baseline["certificateTrustLocations"]}
current_trust_set = {(item["store"], item["thumbprint"].upper()) for item in trust_locations}
unchanged = all(item["unchanged"] for item in artifacts + manifests)
cert_unchanged = current_cert_set == prior_cert_set
trust_unchanged = current_trust_set == prior_trust_set
result = {"at": datetime.now(timezone.utc).isoformat(), "baseline": str(baseline_path),
          "baselineSha256": observe(baseline_path)["sha256"], "kind": "read-only closing consistency snapshot",
          "artifactPaths": artifacts, "manifests": manifests, "certificates": certificates,
          "certificateReadProcess": {"pid": process.pid, "exit": process.returncode, "waited": True, "stdoutEOF": True, "stderrEOF": True},
          "certificateFingerprintSetUnchanged": cert_unchanged,
          "certificateTrustLocations": trust_locations,
          "certificateTrustLocationsUnchanged": trust_unchanged,
          "addedTrustLocations": sorted(current_trust_set-prior_trust_set),
          "removedTrustLocations": sorted(prior_trust_set-current_trust_set),
          "addedCertificateStoreAndThumbprint": sorted(current_cert_set - prior_cert_set),
          "removedCertificateStoreAndThumbprint": sorted(prior_cert_set - current_cert_set),
          "newArtifactPathsInOriginalDiscoveryScope": [{"path": path, "current": observe(path)} for path in new_paths],
          "newPackageManifestsInOriginalDiscoveryScope": [{"path": path, "current": observe(path)} for path in new_manifests],
          "allOriginalBytesUnchanged": unchanged,
          "priorOfflineUnsignedResultsReusableForSameBytes": unchanged and cert_unchanged and trust_unchanged and not new_paths and not new_manifests,
          "winVerifyTrustRerun": False, "productExecutions": 0, "networkRequests": 0,
          "preservedToolingFailures": ["First metadata child failed resolving Certificate provider dynamic -CodeSigningCert parameter before returning metadata; exited 1.", "Second metadata child explicit module import failed AuthorizationManager on a portable Codex runtime Security.types.ps1xml; exited 1. No metadata result was inferred from either failure.", "Final collector uses the explicit Windows system PowerShell executable and .NET X509Store ReadOnly|OpenExistingOnly; no provider import, execution-policy override, trust change, signing or product action. No WinVerifyTrust checks were repeated."],
          "limits": ["No global atomic snapshot; reports describe these bounded read intervals.",
                     "Baseline manifest lengths were not stored; this readback records lengths and compares the exact prior SHA256.",
                     "Certificate equality covers code-signing fingerprints in CU/LM My and those fingerprints in CU/LM Root, TrustedPublisher and TrustedPeople, not unrelated trust-store entries.",
                     "No signing, key access, key creation/export, trust changes or online revocation checks.",
                     "No new signature result is claimed for changed/new bytes; those require a targeted offline follow-up."]}
with output.open("x", encoding="utf-8") as file:
    json.dump(result, file, ensure_ascii=False, indent=2)
    file.write("\n")
print(json.dumps({"report": str(output), "artifactPaths": len(artifacts), "manifests": len(manifests),
                  "allOriginalBytesUnchanged": unchanged, "certificateSetUnchanged": cert_unchanged,
                  "certificateTrustLocationsUnchanged": trust_unchanged,
                  "newArtifactPaths": new_paths, "newManifests": new_manifests,
                  "priorOfflineUnsignedResultsReusableForSameBytes": result["priorOfflineUnsignedResultsReusableForSameBytes"]}, ensure_ascii=False))
