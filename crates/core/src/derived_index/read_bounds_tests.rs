use super::*;
use std::time::Duration;

fn sample() -> Data {
    Data {
        head: Head {
            ledger: "bounded-read".into(),
            count: 1,
            hash: "hash".into(),
            bindings: "bindings".into(),
        },
        events: vec![IndexedEvent {
            sequence: 1,
            operation_id: "op1".into(),
            kind: "test".into(),
            received_at: "2026-10-23T00:00:00Z".into(),
            hash: "hash".into(),
            task: Some("t1".into()),
            attempt: None,
        }],
        plans: vec![Plan {
            id: "p1".into(),
            revision: 1,
            title: "test".into(),
            tasks: vec!["task".into()],
            digest: "digest".into(),
            created_at: "2026-10-23T00:00:00Z".into(),
            approved: false,
        }],
        summaries: vec![EventSummary {
            sequence: 1,
            kind: "test".into(),
            at: "2026-10-23T00:00:00Z".into(),
            summary: "summary".into(),
        }],
        tasks: vec![IndexedTask {
            id: "t1".into(),
            state: "unknown".into(),
            attempt: None,
            first_day: "2026-10-22".into(),
            updated_at: "2026-10-23T00:00:00Z".into(),
            unfinished: true,
        }],
        daily: vec![DailyVolume {
            day: "2026-10-23".into(),
            path: "daily.md".into(),
            number: 1,
            sealed: false,
            event_ids: vec!["op1".into()],
            markdown: "test".into(),
        }],
    }
}
fn prepared() -> (tempfile::TempDir, Cache, Data) {
    let root = tempfile::tempdir().unwrap();
    let data = sample();
    let mut cache = Cache::default();
    cache.refresh(
        root.path(),
        &DataRef {
            head: &data.head,
            events: &data.events,
            plans: &data.plans,
            summaries: &data.summaries,
            tasks: &data.tasks,
            daily: &data.daily,
        },
    );
    cache.wait(Duration::from_secs(20)).unwrap();
    (root, cache, data)
}
fn edit(root: &Path, sql: &str, argument: &str) {
    Connection::open(root.join("index.sqlite"))
        .unwrap()
        .execute(sql, [argument])
        .unwrap();
}

#[test]
fn borrowed_payload_limit_covers_full_pages_daily_and_unfinished() {
    let (root, cache, data) = prepared();
    assert_eq!(
        fingerprint(&cache.read(root.path(), &data.head).unwrap()).unwrap(),
        fingerprint(&data).unwrap()
    );
    for (table, property, query) in [
        ("plans", "title", 0),
        ("summaries", "summary", 1),
        ("daily_volumes", "padding", 2),
        ("tasks", "padding", 3),
    ] {
        publish(root.path(), &data).unwrap();
        let large = "x".repeat(memory::SNAPSHOT);
        edit(
            root.path(),
            &format!("UPDATE {table} SET payload=json_set(payload,'$.{property}',?1)"),
            &large,
        );
        let error = match query {
            0 => cache.plans(root.path(), &data.head, 0, 1).unwrap_err(),
            1 => cache.summaries(root.path(), &data.head, 0, 1).unwrap_err(),
            2 => cache.daily(root.path(), &data.head).unwrap_err(),
            _ => cache
                .unfinished_before(root.path(), &data.head, "2026-10-24")
                .unwrap_err(),
        };
        assert!(error.contains("byte limit"), "{table}: {error}");
        assert!(cache.read(root.path(), &data.head).is_err());
    }
}

#[test]
fn complete_read_shares_payload_budget_across_tables() {
    let (root, cache, data) = prepared();
    let padding = "x".repeat(9 * 1024 * 1024);
    for table in ["plans", "summaries"] {
        edit(
            root.path(),
            &format!("UPDATE {table} SET payload=json_set(payload,'$.padding',?1)"),
            &padding,
        );
    }
    // Each row fits, including extra ignored JSON fields; together they do not.
    assert_eq!(
        cache.plans(root.path(), &data.head, 0, 1).unwrap(),
        data.plans
    );
    assert_eq!(
        cache.summaries(root.path(), &data.head, 0, 1).unwrap(),
        data.summaries
    );
    let error = cache.read(root.path(), &data.head).unwrap_err();
    assert!(error.contains("byte limit"), "{error}");
}

#[test]
fn excess_rows_are_rejected_before_deserializing_an_extra_payload() {
    let (root, cache, data) = prepared();
    // The extra payload has correct query columns but wrong Plan field types.
    // Count rejection must precede serde's type rejection.
    Connection::open(root.path().join("index.sqlite"))
        .unwrap()
        .execute_batch("INSERT INTO plans VALUES(1,'p2',0,'{\"id\":\"p2\",\"approved\":0}');")
        .unwrap();
    let error = cache.read(root.path(), &data.head).unwrap_err();
    assert!(error.contains("beyond ledger count"), "{error}");
}

#[test]
fn altered_schema_and_recursive_views_are_rejected_without_repair() {
    let (root, cache, data) = prepared();
    for sql in [
        "DROP TABLE summaries; CREATE VIEW summaries AS WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n) SELECT x AS sequence, '{}' AS payload FROM n;",
        "CREATE INDEX foreign_index ON summaries(payload);",
        "CREATE TRIGGER foreign_trigger AFTER UPDATE ON summaries BEGIN DELETE FROM events; END;",
        "DROP INDEX plans_approved;",
    ] {
        publish(root.path(), &data).unwrap();
        Connection::open(root.path().join("index.sqlite")).unwrap().execute_batch(sql).unwrap();
        let before = fs::read(root.path().join("index.sqlite")).unwrap();
        let error = cache.plans(root.path(), &data.head, 0, 1).unwrap_err();
        assert!(error.contains("schema"), "{error}");
        assert_eq!(fs::read(root.path().join("index.sqlite")).unwrap(), before);
    }
}

#[test]
fn metadata_text_and_type_are_bounded_before_allocation() {
    let (root, cache, data) = prepared();
    for (column, value) in [("head", "x".repeat(32769)), ("digest", "x".repeat(65))] {
        publish(root.path(), &data).unwrap();
        edit(root.path(), &format!("UPDATE meta SET {column}=?1"), &value);
        assert!(cache
            .plans(root.path(), &data.head, 0, 1)
            .unwrap_err()
            .contains("byte limit"));
    }
    publish(root.path(), &data).unwrap();
    Connection::open(root.path().join("index.sqlite"))
        .unwrap()
        .execute_batch("UPDATE meta SET head=zeroblob(10)")
        .unwrap();
    assert!(cache
        .plans(root.path(), &data.head, 0, 1)
        .unwrap_err()
        .contains("text type"));
}

#[test]
fn native_value_limit_and_vm_budget_interrupt_without_global_settings() {
    let conn = Connection::open_in_memory().unwrap();
    read_bounds::configure(&conn).unwrap();
    let error = conn
        .query_row("SELECT zeroblob(?1)", [64 * 1024 * 1024], |r| {
            r.get::<_, Vec<u8>>(0)
        })
        .unwrap_err();
    assert_eq!(error.sqlite_error_code(), Some(rusqlite::ErrorCode::TooBig));
    let error = conn.query_row("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000000) SELECT sum(x) FROM n", [], |r| r.get::<_, i64>(0)).unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::OperationInterrupted)
    );
    let other = Connection::open_in_memory().unwrap();
    assert_eq!(
        other
            .query_row("SELECT length(zeroblob(?1))", [64 * 1024 * 1024], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        64 * 1024 * 1024
    );
    assert!(conn.execute_batch("CREATE TABLE forbidden(x)").is_err());
}

#[test]
fn oversized_cache_falls_back_and_rebuilds_without_adding_events() {
    let root = tempfile::tempdir().unwrap();
    let mut store = crate::Store::open(root.path()).unwrap();
    store
        .execute(cerulean_domain::Command::CreatePlan {
            title: "bounded cache".into(),
            tasks: vec!["test".into()],
        })
        .unwrap();
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    let plans = store.snapshot().plans;
    let event_files = || {
        let mut files = fs::read_dir(root.path().join("events"))
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), fs::read(entry.path()).unwrap())
            })
            .collect::<Vec<_>>();
        files.sort();
        files
    };
    let events = event_files();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(root.path().join("index.sqlite"))
        .unwrap();
    file.set_len(read_bounds::FILE_BYTES + 1).unwrap();
    drop(file);
    let error = store
        .index
        .read(root.path(), &store.index.expected.clone().unwrap())
        .unwrap_err();
    assert!(error.contains("file byte limit"), "{error}");
    assert_eq!(store.snapshot().plans, plans);
    store.refresh_index();
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    assert!(
        fs::metadata(root.path().join("index.sqlite"))
            .unwrap()
            .len()
            < read_bounds::FILE_BYTES
    );
    assert_eq!(store.snapshot().plans, plans);
    assert_eq!(event_files(), events);
}
