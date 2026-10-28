//! Disposable SQLite reading views. Never used to authorize a command or replay a ledger.
use crate::handoff::DailyVolume;
use cerulean_domain::{EventSummary, Plan};
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
mod memory;
mod read_bounds;
mod worker;
pub(crate) use worker::Diagnostics;

/// Optional handoff memoization shares the retained-data budget with workers.
pub(crate) struct RetainedCapacity(memory::Lease);
impl RetainedCapacity {
    pub fn resize(&mut self, bytes: usize) -> Result<(), String> {
        if bytes > memory::SNAPSHOT {
            return Err("handoff memo exceeds retained byte limit".into());
        }
        self.0.resize(bytes)
    }
}

#[cfg(test)]
pub(crate) fn wait_for_test_projections(root: &Path) {
    worker::wait_for_test_root(root);
}

/// A disposable workload. Document publication and SQLite failure are independent.
pub(super) struct Workload {
    head: Head,
    index: Result<memory::Encoded, String>,
    documents: Option<Result<memory::Encoded, String>>,
    checkpoint: Option<Result<memory::Encoded, String>>,
}

const SCHEMA: u32 = 1;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedEvent {
    pub sequence: u64,
    pub operation_id: String,
    pub kind: String,
    pub received_at: String,
    pub hash: String,
    pub task: Option<String>,
    pub attempt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedTask {
    pub id: String,
    pub state: String,
    pub attempt: Option<String>,
    pub first_day: String,
    pub updated_at: String,
    pub unfinished: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Head {
    pub ledger: String,
    pub count: usize,
    pub hash: String,
    pub bindings: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Data {
    pub head: Head,
    pub events: Vec<IndexedEvent>,
    pub plans: Vec<Plan>,
    pub summaries: Vec<EventSummary>,
    pub tasks: Vec<IndexedTask>,
    pub daily: Vec<DailyVolume>,
}

#[derive(Serialize)]
pub(crate) struct DataRef<'a, E: ?Sized = [IndexedEvent]> {
    pub head: &'a Head,
    pub events: &'a E,
    pub plans: &'a [Plan],
    pub summaries: &'a [EventSummary],
    pub tasks: &'a [IndexedTask],
    pub daily: &'a [DailyVolume],
}

#[derive(Default)]
pub(crate) struct Cache {
    expected: Option<Head>,
    error: Option<String>,
    worker: worker::Worker,
    checkpoint: Option<Result<memory::Encoded, String>>,
}

struct Expected {
    head: Head,
    digest: String,
    plans: Vec<String>,
    summaries: Vec<String>,
    daily: Vec<String>,
    tasks: Vec<(String, bool, String)>,
    _memory: Option<memory::Lease>,
}
fn row_digest<T: Serialize>(row: &T) -> Result<String, String> {
    Ok(format!("{:x}", Sha256::digest(json(row)?.as_bytes())))
}
impl Expected {
    fn new(data: &Data, budget: &std::sync::Arc<memory::Budget>) -> Result<Self, String> {
        let mut expected = Self {
            head: data.head.clone(),
            digest: fingerprint(data)?,
            plans: data
                .plans
                .iter()
                .map(row_digest)
                .collect::<Result<_, _>>()?,
            summaries: data
                .summaries
                .iter()
                .map(row_digest)
                .collect::<Result<_, _>>()?,
            daily: data
                .daily
                .iter()
                .map(row_digest)
                .collect::<Result<_, _>>()?,
            tasks: data
                .tasks
                .iter()
                .map(|t| Ok((t.first_day.clone(), t.unfinished, row_digest(t)?)))
                .collect::<Result<_, String>>()?,
            _memory: None,
        };
        let strings = |values: &Vec<String>| {
            values.capacity() * std::mem::size_of::<String>()
                + values.iter().map(String::capacity).sum::<usize>()
        };
        let bytes = std::mem::size_of::<Self>()
            + expected.head.ledger.capacity()
            + expected.head.hash.capacity()
            + expected.head.bindings.capacity()
            + expected.digest.capacity()
            + strings(&expected.plans)
            + strings(&expected.summaries)
            + strings(&expected.daily)
            + expected.tasks.capacity() * std::mem::size_of::<(String, bool, String)>()
            + expected
                .tasks
                .iter()
                .map(|(day, _, proof)| day.capacity() + proof.capacity())
                .sum::<usize>();
        expected._memory = Some(budget.reserve(bytes)?);
        Ok(expected)
    }
}

fn json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}
fn fingerprint(data: &Data) -> Result<String, String> {
    struct HashWriter(Sha256);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, data).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", writer.0.finalize()))
}
fn db<T>(result: rusqlite::Result<T>) -> Result<T, String> {
    result.map_err(|e| format!("derived index: {e}"))
}

fn regular(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("derived index refuses reparse points".into());
                }
            }
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err("derived index requires an ordinary file".into());
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("derived index metadata: {e}")),
    }
}

fn check_paths(root: &Path) -> Result<(), String> {
    if regular(&root.join("index.sqlite"))? {
        #[cfg(windows)]
        let _pin = crate::backup::open(&root.join("index.sqlite"), false, false)
            .map_err(|e| e.to_string())?;
    }
    // Our immutable publications never create sidecars. Leave unexpected files
    // untouched and use the ledger view instead of recovering untrusted SQLite.
    for suffix in ["-journal", "-wal", "-shm"] {
        if regular(&root.join(format!("index.sqlite{suffix}")))? {
            return Err("unexpected SQLite sidecar; ledger fallback active".into());
        }
    }
    Ok(())
}

impl Cache {
    #[cfg(test)]
    pub(crate) fn test_with_capacity(bytes: usize) -> Self {
        Self {
            worker: worker::Worker::with_pool(std::sync::Arc::new(worker::pool::Pool::with_bytes(
                1, 8, bytes,
            ))),
            ..Self::default()
        }
    }
    pub fn retain_handoffs(&self, bytes: usize) -> Result<RetainedCapacity, String> {
        if bytes > memory::SNAPSHOT {
            return Err("handoff memo exceeds retained byte limit".into());
        }
        self.worker.reserve(bytes).map(RetainedCapacity)
    }
    pub fn checkpoint(&mut self, checkpoint: &impl Serialize) {
        self.checkpoint = Some(self.worker.encode(checkpoint, memory::CHECKPOINT));
    }
    pub fn prepare_refresh(&mut self) {
        self.worker.discard_pending_views();
    }
    #[cfg(test)]
    pub fn test_gate(&self) -> worker::GateLease {
        self.worker.test_gate()
    }
    #[cfg(test)]
    pub fn test_stats(&self) -> (usize, Option<usize>, bool) {
        self.worker.test_stats()
    }
    pub fn error(&self) -> Option<String> {
        self.error.clone().or_else(|| {
            self.expected
                .as_ref()
                .and_then(|head| self.worker.failure(head))
        })
    }

    pub fn refresh<E: Serialize + ?Sized>(&mut self, root: &Path, data: &DataRef<'_, E>) {
        self.expected = Some(data.head.clone());
        let encoded = self.worker.encode(data, memory::SNAPSHOT);
        let result = self.worker.submit(
            root,
            Workload {
                head: data.head.clone(),
                index: encoded,
                documents: None,
                checkpoint: self.checkpoint.take(),
            },
        );
        self.error = result.err();
    }
    pub fn refresh_workflow<E: Serialize>(
        &mut self,
        root: &Path,
        head: Head,
        index: Result<DataRef<'_, E>, String>,
        documents: crate::scheduler::projection::SnapshotRef<'_>,
    ) {
        self.expected = Some(head.clone());
        let index = index.and_then(|data| self.worker.encode(&data, memory::SNAPSHOT));
        let documents = self.worker.encode(&documents, memory::SNAPSHOT);
        self.error = self
            .worker
            .submit(
                root,
                Workload {
                    head,
                    index,
                    documents: Some(documents),
                    checkpoint: self.checkpoint.take(),
                },
            )
            .err();
    }
    pub fn diagnostics(&self) -> Diagnostics {
        self.worker.diagnostics()
    }
    pub fn projection_error(&self) -> Option<String> {
        self.error
            .clone()
            .or_else(|| self.worker.projection_error())
    }
    pub fn wait_for_projections(&self, timeout: std::time::Duration) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.worker.wait_documents(
            self.expected.as_ref().ok_or("projections unavailable")?,
            timeout,
        )
    }
    fn ready(&self, head: &Head) -> Result<std::sync::Arc<Expected>, String> {
        if self.expected.as_ref() != Some(head) {
            return Err("index head is stale".into());
        }
        self.worker.ready(head)
    }
    pub fn wait(&self, timeout: std::time::Duration) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.worker
            .wait(self.expected.as_ref().ok_or("index unavailable")?, timeout)
    }
    pub fn shutdown(&mut self) {
        self.worker.shutdown();
    }
    pub fn read(&self, root: &Path, head: &Head) -> Result<Data, String> {
        read_complete(root, self.ready(head)?.as_ref())
    }
    pub fn plans(
        &self,
        root: &Path,
        head: &Head,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Plan>, String> {
        let expected = self.ready(head)?;
        read_page(root, &expected, Table::Plans, offset, limit)
    }
    pub fn summaries(
        &self,
        root: &Path,
        head: &Head,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<EventSummary>, String> {
        let expected = self.ready(head)?;
        read_page(root, &expected, Table::Summaries, offset, limit)
    }
    pub fn daily(&self, root: &Path, head: &Head) -> Result<Vec<DailyVolume>, String> {
        let expected = self.ready(head)?;
        let (_pin, conn) = connection(root, &expected, false)?;
        let values:Vec<DailyVolume> = read_bounds::rows(&conn,"SELECT payload FROM daily_volumes WHERE day=json_extract(payload,'$.day') AND path=json_extract(payload,'$.path') ORDER BY ordinal LIMIT ?1", expected.daily.len(), &mut read_bounds::PayloadBudget::default())?;
        verify_rows(&values, &expected.daily)?;
        Ok(values)
    }
    pub fn unfinished_before(
        &self,
        root: &Path,
        head: &Head,
        day: &str,
    ) -> Result<Vec<IndexedTask>, String> {
        let expected = self.ready(head)?;
        let (_pin, conn) = connection(root, &expected, false)?;
        let selected: Vec<_> = expected
            .tasks
            .iter()
            .enumerate()
            .filter(|(_, (first, unfinished, _))| *unfinished && first.as_str() < day)
            .collect();
        // Scan in primary-key order, avoiding a temporary sort of untrusted rows.
        let mut statement=db(conn.prepare("SELECT ordinal,payload FROM tasks NOT INDEXED WHERE unfinished=1 AND first_day<?1 ORDER BY ordinal LIMIT ?2"))?;
        let mut rows = db(statement.query(params![day, read_bounds::row_limit(selected.len())?]))?;
        let mut budget = read_bounds::PayloadBudget::default();
        let mut values = Vec::new();
        while let Some(row) = db(rows.next())? {
            let (wanted, (_, _, proof)) = selected
                .get(values.len())
                .ok_or("unexpected indexed task")?;
            let ordinal = db(row.get::<_, i64>(0))?;
            let task: IndexedTask = budget.decode(row, 1)?;
            if usize::try_from(ordinal).ok() != Some(*wanted) || row_digest(&task)? != *proof {
                return Err("indexed task differs from ledger".into());
            }
            values.push(task);
        }
        if values.len() != selected.len() {
            return Err("indexed task omitted".into());
        }
        Ok(values)
    }
}

enum Table {
    Plans,
    Summaries,
}
fn verify_rows<T: Serialize>(values: &[T], proofs: &[String]) -> Result<(), String> {
    if values.len() != proofs.len() {
        return Err("index row count differs from ledger".into());
    }
    for (value, proof) in values.iter().zip(proofs) {
        if row_digest(value)? != *proof {
            return Err("index row differs from ledger".into());
        }
    }
    Ok(())
}
fn read_page<T: serde::de::DeserializeOwned + Serialize>(
    root: &Path,
    expected: &Expected,
    table: Table,
    offset: usize,
    limit: usize,
) -> Result<Vec<T>, String> {
    if limit == 0 || limit > 256 {
        return Err("index page limit outside 1..256".into());
    }
    let (sql,proofs)=match table {
        Table::Plans=>("SELECT payload FROM plans WHERE ordinal>=?2 AND ordinal<?3 AND id=json_extract(payload,'$.id') AND approved=json_extract(payload,'$.approved') ORDER BY ordinal LIMIT ?1",&expected.plans),
        Table::Summaries=>("SELECT payload FROM summaries WHERE sequence>=?2+1 AND sequence<?3+1 AND sequence=json_extract(payload,'$.sequence') ORDER BY sequence LIMIT ?1",&expected.summaries),
    };
    if offset > proofs.len() {
        return Err("index offset outside ledger".into());
    }
    let (_pin, conn) = connection(root, expected, false)?;
    let mut statement = db(conn.prepare(sql))?;
    let mut rows = db(statement.query(params![
        limit as i64,
        i64::try_from(offset).map_err(|_| "index offset overflow")?,
        i64::try_from(offset.saturating_add(limit)).map_err(|_| "index upper bound overflow")?
    ]))?;
    let mut budget = read_bounds::PayloadBudget::default();
    let mut values: Vec<T> = Vec::new();
    while let Some(row) = db(rows.next())? {
        if values.len() >= proofs.len().saturating_sub(offset).min(limit) {
            return Err("unexpected index page row".into());
        }
        values.push(budget.decode(row, 0)?);
    }
    verify_rows(
        &values,
        &proofs[offset..proofs.len().min(offset.saturating_add(limit))],
    )?;
    Ok(values)
}

fn connection(
    root: &Path,
    expected: &Expected,
    check: bool,
) -> Result<(Option<fs::File>, Connection), String> {
    check_paths(root)?;
    let path = root.join("index.sqlite");
    #[cfg(windows)]
    let file = crate::backup::open(&path, false, false).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let file = fs::File::open(&path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > read_bounds::FILE_BYTES {
        return Err("derived index file byte limit exceeded".into());
    }
    let pin = Some(file);
    let conn = db(Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    ))?;
    read_bounds::configure(&conn)?;
    read_bounds::schema(&conn)?;
    if check {
        let mut statement = db(conn.prepare("PRAGMA quick_check(1)"))?;
        let mut rows = db(statement.query([]))?;
        let row = db(rows.next())?.ok_or("index integrity result missing")?;
        if read_bounds::text(row, 0, 256)? != b"ok" {
            return Err("derived index integrity check failed".into());
        }
    }
    read_bounds::meta(&conn, expected)?;
    Ok((pin, conn))
}
fn read_complete(root: &Path, expected: &Expected) -> Result<Data, String> {
    let (_pin, conn) = connection(root, expected, true)?;
    let mut budget = read_bounds::PayloadBudget::default();
    let data = Data {
            head: expected.head.clone(),
            events: read_bounds::rows(&conn, "SELECT payload FROM events WHERE sequence=json_extract(payload,'$.sequence') AND operation_id=json_extract(payload,'$.operation_id') AND kind=json_extract(payload,'$.kind') AND received_at=json_extract(payload,'$.received_at') AND hash=json_extract(payload,'$.hash') AND task IS json_extract(payload,'$.task') AND attempt IS json_extract(payload,'$.attempt') ORDER BY sequence LIMIT ?1", expected.head.count, &mut budget)?,
            plans: read_bounds::rows(&conn, "SELECT payload FROM plans WHERE id=json_extract(payload,'$.id') AND approved=json_extract(payload,'$.approved') ORDER BY ordinal LIMIT ?1", expected.plans.len(), &mut budget)?,
            summaries: read_bounds::rows(&conn, "SELECT payload FROM summaries WHERE sequence=json_extract(payload,'$.sequence') ORDER BY sequence LIMIT ?1", expected.summaries.len(), &mut budget)?,
            tasks: read_bounds::rows(&conn, "SELECT payload FROM tasks WHERE id=json_extract(payload,'$.id') AND state=json_extract(payload,'$.state') AND first_day=json_extract(payload,'$.first_day') AND unfinished=json_extract(payload,'$.unfinished') ORDER BY ordinal LIMIT ?1", expected.tasks.len(), &mut budget)?,
            daily: read_bounds::rows(&conn, "SELECT payload FROM daily_volumes WHERE day=json_extract(payload,'$.day') AND path=json_extract(payload,'$.path') ORDER BY ordinal LIMIT ?1", expected.daily.len(), &mut budget)?,
        };
    // A matching head alone cannot detect altered/deleted cached rows.
    if fingerprint(&data)? != expected.digest {
        return Err("index content differs from validated ledger".into());
    }
    Ok(data)
}

fn build(
    root: &Path,
    data: &Data,
    budget: &std::sync::Arc<memory::Budget>,
) -> Result<Expected, String> {
    let expected = Expected::new(data, budget)?;
    if read_complete(root, &expected).is_err() {
        publish(root, data)?;
        read_complete(root, &expected)?;
    }
    Ok(expected)
}

fn publish(root: &Path, data: &Data) -> Result<(), String> {
    check_paths(root)?;
    let destination = root.join("index.sqlite");
    let temp = root.join(format!(
        "index-{}-{}.sqlite.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let mut conn = db(Connection::open_with_flags(
            &temp,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        ))?;
        db(conn.busy_timeout(std::time::Duration::ZERO))?;
        // Only the private, unpublished database uses an in-memory rollback journal.
        db(conn.execute_batch("PRAGMA journal_mode=MEMORY; PRAGMA synchronous=FULL;"))?;
        for (_, _, _, sql) in read_bounds::SCHEMA_OBJECTS {
            if let Some(sql) = sql {
                db(conn.execute_batch(sql))?;
            }
        }
        let tx = db(conn.transaction())?;
        db(tx.execute(
            "INSERT INTO meta VALUES(1,?1,?2,?3)",
            params![SCHEMA, json(&data.head)?, fingerprint(data)?],
        ))?;
        for e in &data.events {
            db(tx.execute(
                "INSERT INTO events VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    i64::try_from(e.sequence).map_err(|_| "index sequence overflow")?,
                    e.operation_id,
                    e.kind,
                    e.received_at,
                    e.hash,
                    e.task,
                    e.attempt,
                    json(e)?
                ],
            ))?;
        }
        for (i, p) in data.plans.iter().enumerate() {
            db(tx.execute(
                "INSERT INTO plans VALUES(?1,?2,?3,?4)",
                params![
                    i64::try_from(i).map_err(|_| "index ordinal overflow")?,
                    p.id,
                    p.approved,
                    json(p)?
                ],
            ))?;
        }
        for e in &data.summaries {
            db(tx.execute(
                "INSERT INTO summaries VALUES(?1,?2)",
                params![e.sequence, json(e)?],
            ))?;
        }
        for (i, t) in data.tasks.iter().enumerate() {
            db(tx.execute(
                "INSERT INTO tasks VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    i64::try_from(i).map_err(|_| "index ordinal overflow")?,
                    t.id,
                    t.state,
                    t.first_day,
                    t.unfinished,
                    json(t)?
                ],
            ))?;
        }
        for (i, v) in data.daily.iter().enumerate() {
            db(tx.execute(
                "INSERT INTO daily_volumes VALUES(?1,?2,?3,?4)",
                params![
                    i64::try_from(i).map_err(|_| "index ordinal overflow")?,
                    v.day,
                    v.path,
                    json(v)?
                ],
            ))?;
        }
        db(tx.commit())?;
        conn.close().map_err(|(_, e)| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        check_paths(root)?;
        crate::directory_publication::replace(&temp, &destination).map_err(|e| e.to_string())
    })();
    drop(file);
    // Only remove the private temp we created; never remove a foreign sidecar.
    if result.is_err() && regular(&temp) == Ok(true) {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod async_tests;
#[cfg(test)]
mod memory_tests;
#[cfg(test)]
mod pool_tests;
#[cfg(test)]
mod read_bounds_tests;
#[cfg(test)]
mod tests;
