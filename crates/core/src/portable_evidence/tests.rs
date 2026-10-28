use super::*;
use serde_json::json;

fn setup(area: &Path) -> (std::path::PathBuf, std::path::PathBuf, String) {
    let source = area.join("source");
    fs::create_dir(&source).unwrap();
    fs::create_dir(source.join("logs")).unwrap();
    fs::write(source.join("result.json"), b"{\"realityAttested\":false}\n").unwrap();
    fs::write(source.join("logs/out.txt"), b"original unknown\n").unwrap();
    fs::create_dir(source.join("credentials")).unwrap();
    fs::write(source.join("credentials/private.dpapi"), b"unselected trap").unwrap();
    let files = ["result.json", "logs/out.txt"]
        .into_iter()
        .map(|path| {
            let bytes = fs::read(source.join(path)).unwrap();
            Entry {
                path: path.into(),
                bytes: bytes.len() as u64,
                sha256: hash(&bytes),
            }
        })
        .collect::<Vec<_>>();
    let raw = serde_json::to_vec(&files).unwrap();
    let index = area.join("index.json");
    fs::write(&index, &raw).unwrap();
    (source, index, hash(&raw))
}
#[test]
fn archive_relocates_without_source_or_index_and_has_no_authority() {
    let temp = tempfile::tempdir().unwrap();
    let (source, index, sha) = setup(temp.path());
    let archive = temp.path().join("archive");
    let exported = create(&source, &index, &sha, &archive).unwrap();
    assert_eq!(exported.files_verified, 2);
    let relocated = temp.path().join("relocated");
    assert!(relocated.starts_with(temp.path()));
    fs::rename(&archive, &relocated).unwrap();
    let hidden = temp.path().join("source-unavailable");
    assert!(hidden.starts_with(temp.path()));
    fs::rename(source, hidden).unwrap();
    fs::remove_file(index).unwrap();
    let verified = verify(&relocated, &exported.manifest_sha256).unwrap();
    assert_eq!(verified.bytes_verified, exported.bytes_verified);
    assert!(
        !verified.source_workspace_required
            && !verified.execution_allowed
            && !verified.restorable_workbench
            && !verified.reality_attested
            && !verified.release_accepted
    );
    assert!(!relocated.join("payload/credentials").exists());
    assert!(verify(&relocated, &"0".repeat(64)).is_err());
}
#[test]
fn corruption_extra_entries_and_partial_publication_refuse_verification() {
    for case in [
        "content",
        "missing",
        "extra",
        "empty-dir",
        "pending",
        "claim",
        "duplicate-field",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (source, index, sha) = setup(temp.path());
        let archive = temp.path().join("archive");
        let exported = create(&source, &index, &sha, &archive).unwrap();
        let mut selected = exported.manifest_sha256;
        match case {
            "content" => {
                fs::write(archive.join("payload/logs/out.txt"), b"forged success").unwrap()
            }
            "missing" => fs::remove_file(archive.join("payload/result.json")).unwrap(),
            "extra" => fs::write(archive.join("payload/extra.txt"), b"unexpected").unwrap(),
            "empty-dir" => fs::create_dir(archive.join("payload/extra")).unwrap(),
            "pending" => fs::write(archive.join("manifest.pending"), b"uncertain").unwrap(),
            "claim" => {
                let mut m: serde_json::Value =
                    serde_json::from_slice(&fs::read(archive.join("manifest.json")).unwrap())
                        .unwrap();
                m["executionAllowed"] = json!(true);
                let raw = serde_json::to_vec(&m).unwrap();
                selected = hash(&raw);
                fs::write(archive.join("manifest.json"), raw).unwrap();
            }
            "duplicate-field" => {
                let raw = fs::read_to_string(archive.join("manifest.json"))
                    .unwrap()
                    .replacen('{', "{\"schema\":1,", 1);
                selected = hash(raw.as_bytes());
                fs::write(archive.join("manifest.json"), raw).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(verify(&archive, &selected).is_err(), "{case}");
    }
}
#[test]
fn invalid_paths_aliases_credentials_and_inventory_shapes_fail_before_write() {
    for name in [
        "../escape",
        "/absolute",
        "C:/absolute",
        "a\\b",
        "a/../b",
        "a//b",
        "file:stream",
        "CON.txt",
        "com¹.txt",
        "bad*name",
        ".env",
        "credentials/private.dpapi",
        "private.pem",
        "a/尾部.",
    ] {
        assert!(safe_name(name).is_err(), "{name}");
    }
    let entry = |path: &str| Entry {
        path: path.into(),
        bytes: 0,
        sha256: hash(b""),
    };
    for files in [
        vec![entry("x"), entry("X")],
        vec![entry("a"), entry("a/b")],
        vec![entry("A/x"), entry("a/y")],
    ] {
        assert!(inventory(&files).is_err());
    }
    assert!(parse::<Vec<Entry>>(br#"[{"path":"x","bytes":true,"sha256":"a"}]"#).is_err());
    let temp = tempfile::tempdir().unwrap();
    let (source, index, _) = setup(temp.path());
    let bad = serde_json::to_vec(&vec![entry("credentials/private.dpapi")]).unwrap();
    fs::write(&index, &bad).unwrap();
    let to = temp.path().join("archive");
    assert!(create(&source, &index, &hash(&bad), &to).is_err());
    assert!(!to.exists());
}
#[test]
fn changed_source_leaves_pending_without_overwrite_or_source_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let (source, index, sha) = setup(temp.path());
    let to = temp.path().join("archive");
    fs::write(source.join("logs/out.txt"), b"changed before copy").unwrap();
    let original = fs::read(source.join("result.json")).unwrap();
    assert!(create(&source, &index, &sha, &to).is_err());
    assert!(to.join("manifest.pending").is_file());
    assert!(!to.join("manifest.json").exists());
    assert!(create(&source, &index, &sha, &to).is_err());
    assert!(verify(&to, &"0".repeat(64)).is_err());
    assert_eq!(fs::read(source.join("result.json")).unwrap(), original);
    assert!(create(&source, &index, &sha, &source.join("nested")).is_err());
    assert!(!source.join("nested").exists());
}
#[test]
fn hardlinks_junctions_and_concurrent_file_writes_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let (source, index, sha) = setup(temp.path());
    let alias = temp.path().join("hardlink");
    fs::hard_link(source.join("result.json"), &alias).unwrap();
    assert!(create(&source, &index, &sha, &temp.path().join("hardlink-archive")).is_err());
    fs::remove_file(alias).unwrap();
    let link = temp.path().join("junction");
    let output = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&source)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(create(&link, &index, &sha, &temp.path().join("junction-archive")).is_err());
    fs::remove_dir(&link).unwrap();
    let _pin = backup::open(&source.join("result.json"), false, false).unwrap();
    assert!(fs::write(source.join("result.json"), b"blocked").is_err());
}
