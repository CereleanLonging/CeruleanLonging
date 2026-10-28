use super::*;
use crate::management_execution::tests::{intent, response};
use crate::reader_generation::tests::snapshot;
use std::time::Instant;

fn seed(root: &Path, count: usize) -> DecisionJournal {
    let mut journal = DecisionJournal::open(root).unwrap();
    for n in 0..count {
        let mut i = intent();
        i.operation_id = format!("batch-{n:02}");
        journal.begin(i.clone()).unwrap();
        journal.finish(&i.operation_id, Ok(response(&i))).unwrap();
        let head = journal.evidence_digest(&i.operation_id).unwrap().to_owned();
        journal
            .resolve(
                &i.operation_id,
                &head,
                UserResolution::Discard {
                    reason: "controlled original closed record".into(),
                },
            )
            .unwrap();
    }
    journal
}

#[test]
fn historical_batch_matches_original_digest_and_reduces_measured_journal_io() {
    let temp = tempfile::tempdir().unwrap();
    let journal = seed(temp.path(), 21);
    let ids: Vec<_> = journal.operations.keys().map(String::as_str).collect();
    let before = snapshot(temp.path());
    read_metrics::take();
    let start = Instant::now();
    let single: BTreeMap<_, _> = ids
        .iter()
        .map(|id| {
            (
                (*id).to_owned(),
                journal.historical_source(id, true).unwrap(),
            )
        })
        .collect();
    let single_elapsed = start.elapsed();
    let single_counts = read_metrics::take();
    let start = Instant::now();
    let batch = journal.historical_sources(&ids, true).unwrap();
    let batch_elapsed = start.elapsed();
    let batch_counts = read_metrics::take();
    assert_eq!(single, batch);
    assert_eq!(single_counts.full_reads, 42);
    assert_eq!(batch_counts.full_reads, 2);
    assert_eq!(single_counts.record_bytes, batch_counts.record_bytes * 21);
    assert_eq!(single_counts.raw_bytes, batch_counts.raw_bytes);
    // Independently reconstruct the original version-1 digest from actual raw
    // files, rather than relying solely on two paths sharing the new code.
    for (id, source) in &batch {
        let mut records = Records::new();
        for entry in fs::read_dir(&journal.root).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            if name.starts_with(&format!("{id}.")) {
                let bytes = fs::read(entry.path()).unwrap();
                records.insert(
                    format!("management-decisions/{name}"),
                    (format!("{:x}", sha2::Sha256::digest(&bytes)), bytes.len()),
                );
            }
        }
        let reviews: Vec<cerulean_domain::ManagementRecordReview> = vec![];
        assert_eq!(
            source.digest,
            digest(&(1, &source.scope, &records, &reviews, 1u32)).unwrap()
        );
    }
    assert!(before == snapshot(temp.path()));
    println!("21 closed management scopes: single_us={} batch_us={} single={single_counts:?} batch={batch_counts:?}; unchanged original digests and files; no model calls", single_elapsed.as_micros(), batch_elapsed.as_micros());
}

#[test]
fn historical_batch_rejects_midread_changes_without_repair_or_partial_results() {
    for mode in ["whitespace", "missing", "unrelated-corrupt", "unknown-name"] {
        let temp = tempfile::tempdir().unwrap();
        let journal = seed(temp.path(), 3);
        let result = journal.historical_sources_inner(&["batch-00", "batch-01"], true, || {
            let file = journal.root.join("batch-00.0.json");
            match mode {
                "whitespace" => {
                    let mut bytes = fs::read(&file).unwrap();
                    bytes.push(b' ');
                    fs::write(file, bytes).unwrap();
                }
                "missing" => fs::remove_file(file).unwrap(),
                "unrelated-corrupt" => {
                    fs::write(journal.root.join("batch-02.0.json"), b"{partial").unwrap()
                }
                _ => fs::write(journal.root.join("unexpected.txt"), b"keep").unwrap(),
            }
        });
        assert!(result.is_err(), "{mode}");
        let after = snapshot(temp.path());
        if mode != "whitespace" {
            assert!(journal.historical_sources(&["batch-00"], true).is_err());
        }
        assert!(after == snapshot(temp.path()));
    }
}

#[test]
fn historical_batch_pending_aliases_keep_original_source_and_publication_guard() {
    let temp = tempfile::tempdir().unwrap();
    let journal = seed(temp.path(), 2);
    let original = journal
        .historical_sources(&["batch-00", "batch-01"], true)
        .unwrap();
    let target = journal.root.join("batch-00.4.json");
    let pending = journal.root.join("batch-00.4.pending");
    fs::rename(&target, &pending).unwrap();
    let journal = DecisionJournal::open(temp.path()).unwrap();
    let before = snapshot(temp.path());
    assert!(journal
        .historical_sources(&["batch-00", "batch-01"], true)
        .is_err());
    assert_eq!(
        journal
            .historical_sources(&["batch-00", "batch-01"], false)
            .unwrap(),
        original
    );
    assert!(before == snapshot(temp.path()));
    let transition = journal
        .historical_sources_inner(&["batch-00", "batch-01"], false, || {
            fs::rename(&pending, &target).unwrap()
        })
        .unwrap();
    assert_eq!(transition, original);
    assert!(target.exists() && !pending.exists());
}

#[test]
fn historical_batch_requires_current_closed_scopes_and_rechecks_each_request() {
    let temp = tempfile::tempdir().unwrap();
    let mut journal = seed(temp.path(), 2);
    let mut i = intent();
    i.operation_id = "open".into();
    journal.begin(i.clone()).unwrap();
    assert!(journal
        .historical_sources(&["batch-00", "open"], true)
        .is_err());
    assert!(journal
        .historical_sources(&["batch-00", "missing"], true)
        .is_err());
    let first = journal.historical_sources(&["batch-00"], true).unwrap();
    let file = journal.root.join("batch-00.0.json");
    let mut bytes = fs::read(&file).unwrap();
    bytes.push(b' ');
    fs::write(&file, &bytes).unwrap();
    let next = journal.historical_sources(&["batch-00"], true).unwrap();
    assert_ne!(first["batch-00"].digest, next["batch-00"].digest);
    assert_eq!(fs::read(file).unwrap(), bytes);
    let other = DecisionJournal::open(temp.path()).unwrap();
    journal.finish("open", Ok(response(&i))).unwrap();
    let head = journal.evidence_digest("open").unwrap().to_owned();
    journal
        .resolve(
            "open",
            &head,
            UserResolution::Discard {
                reason: "close".into(),
            },
        )
        .unwrap();
    assert!(other.historical_sources(&["open"], true).is_err());
}

#[test]
fn historical_batch_empty_and_invalid_inputs_never_create_or_scan_records() {
    let temp = tempfile::tempdir().unwrap();
    let journal = seed(temp.path(), 0);
    fs::remove_dir(&journal.root).unwrap();
    read_metrics::take();
    assert!(journal.historical_sources(&[], true).unwrap().is_empty());
    assert!(journal.historical_sources(&["../escape"], true).is_err());
    assert!(journal
        .historical_sources(&vec!["id"; MAX_OPERATIONS + 1], true)
        .is_err());
    assert_eq!(read_metrics::take().full_reads, 0);
    assert!(!journal.root.exists());
    assert!(journal.historical_sources(&["id"], true).is_err());
    assert!(!journal.root.exists());
}
