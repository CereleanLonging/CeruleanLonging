//! Limits for disposable, potentially modified SQLite views, not process RSS.
use super::{db, memory};
use rusqlite::{limits::Limit, types::ValueRef, Connection, Row};
use serde::de::DeserializeOwned;
use std::time::{Duration, Instant};

pub(super) const FILE_BYTES: u64 = 128 * 1024 * 1024;
const NATIVE_BYTES: i32 = (2 * memory::SNAPSHOT + 65536) as i32;
const SCHEMA_TEXT: usize = 4096;
const HEAD_BYTES: usize = 32768;

// These are the original schema-1 statements. Keep publication and read checks
// together; SQLite stores each statement without its trailing semicolon.
pub(super) const SCHEMA_OBJECTS: &[(&str, &str, &str, Option<&str>)] = &[
    ("table", "meta", "meta", Some("CREATE TABLE meta(id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER, head TEXT, digest TEXT)")),
    ("table", "events", "events", Some("CREATE TABLE events(sequence INTEGER PRIMARY KEY, operation_id TEXT UNIQUE, kind TEXT, received_at TEXT, hash TEXT, task TEXT, attempt TEXT, payload TEXT NOT NULL)")),
    ("index", "events_task", "events", Some("CREATE INDEX events_task ON events(task, attempt, sequence)")),
    ("index", "events_time", "events", Some("CREATE INDEX events_time ON events(received_at, sequence)")),
    ("table", "plans", "plans", Some("CREATE TABLE plans(ordinal INTEGER PRIMARY KEY, id TEXT UNIQUE, approved INTEGER, payload TEXT NOT NULL)")),
    ("index", "plans_approved", "plans", Some("CREATE INDEX plans_approved ON plans(approved)")),
    ("table", "summaries", "summaries", Some("CREATE TABLE summaries(sequence INTEGER PRIMARY KEY, payload TEXT NOT NULL)")),
    ("table", "tasks", "tasks", Some("CREATE TABLE tasks(ordinal INTEGER PRIMARY KEY, id TEXT UNIQUE, state TEXT, first_day TEXT, unfinished INTEGER, payload TEXT NOT NULL)")),
    ("index", "tasks_unfinished", "tasks", Some("CREATE INDEX tasks_unfinished ON tasks(unfinished, first_day)")),
    ("table", "daily_volumes", "daily_volumes", Some("CREATE TABLE daily_volumes(ordinal INTEGER PRIMARY KEY, day TEXT, path TEXT UNIQUE, payload TEXT NOT NULL)")),
    ("index", "daily_day", "daily_volumes", Some("CREATE INDEX daily_day ON daily_volumes(day)")),
    ("index", "sqlite_autoindex_events_1", "events", None),
    ("index", "sqlite_autoindex_plans_1", "plans", None),
    ("index", "sqlite_autoindex_tasks_1", "tasks", None),
    ("index", "sqlite_autoindex_daily_volumes_1", "daily_volumes", None),
];

pub(super) fn configure(conn: &Connection) -> Result<(), String> {
    for (limit, value) in [
        (Limit::SQLITE_LIMIT_LENGTH, NATIVE_BYTES),
        (Limit::SQLITE_LIMIT_SQL_LENGTH, 65536),
        (Limit::SQLITE_LIMIT_COLUMN, 32),
        (Limit::SQLITE_LIMIT_EXPR_DEPTH, 64),
        (Limit::SQLITE_LIMIT_COMPOUND_SELECT, 8),
        (Limit::SQLITE_LIMIT_VDBE_OP, 65536),
        (Limit::SQLITE_LIMIT_FUNCTION_ARG, 16),
        (Limit::SQLITE_LIMIT_ATTACHED, 0),
        (Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 16),
        (Limit::SQLITE_LIMIT_TRIGGER_DEPTH, 0),
        (Limit::SQLITE_LIMIT_WORKER_THREADS, 0),
    ] {
        db(conn.set_limit(limit, value))?;
    }
    // One connection is one read operation. The clock/VM budget is cumulative
    // across schema, metadata, integrity checks and all result tables. SQLite
    // callbacks are cooperative; this is not a hard wall-clock interruption.
    let started = Instant::now();
    let mut callbacks = 0;
    db(conn.progress_handler(
        1000,
        Some(move || {
            callbacks += 1;
            callbacks >= 8000 || started.elapsed() >= Duration::from_secs(2)
        }),
    ))?;
    db(conn.busy_timeout(Duration::ZERO))?;
    db(conn.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; PRAGMA cache_size=-1024; PRAGMA mmap_size=0;"))?;
    Ok(())
}

pub(super) fn text<'a>(row: &'a Row<'_>, column: usize, max: usize) -> Result<&'a [u8], String> {
    match db(row.get_ref(column))? {
        ValueRef::Text(bytes) if bytes.len() <= max => Ok(bytes),
        _ => Err("derived index text type or byte limit exceeded".into()),
    }
}

pub(super) fn schema(conn: &Connection) -> Result<(), String> {
    let mut statement =
        db(conn.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema LIMIT ?1"))?;
    let mut rows = db(statement.query([(SCHEMA_OBJECTS.len() + 1) as i64]))?;
    let mut seen = [false; SCHEMA_OBJECTS.len()];
    while let Some(row) = db(rows.next())? {
        let kind = text(row, 0, 16)?;
        let name = text(row, 1, 128)?;
        let table = text(row, 2, 128)?;
        let sql = match db(row.get_ref(3))? {
            ValueRef::Null => None,
            ValueRef::Text(bytes) if bytes.len() <= SCHEMA_TEXT => Some(bytes),
            _ => return Err("derived index schema text exceeded".into()),
        };
        let index = SCHEMA_OBJECTS
            .iter()
            .position(|(k, n, t, s)| {
                kind == k.as_bytes()
                    && name == n.as_bytes()
                    && table == t.as_bytes()
                    && sql == s.map(str::as_bytes)
            })
            .ok_or("derived index schema objects changed")?;
        if std::mem::replace(&mut seen[index], true) {
            return Err("derived index duplicate schema object".into());
        }
    }
    if seen.iter().any(|found| !found) {
        return Err("derived index schema object missing".into());
    }
    Ok(())
}

pub(super) fn meta(conn: &Connection, expected: &super::Expected) -> Result<(), String> {
    let mut statement = db(conn.prepare("SELECT id,schema_version,head,digest FROM meta LIMIT 2"))?;
    let mut rows = db(statement.query([]))?;
    let row = db(rows.next())?.ok_or("index metadata missing")?;
    if db(row.get::<_, i64>(0))? != 1
        || db(row.get::<_, u32>(1))? != super::SCHEMA
        || text(row, 2, HEAD_BYTES)? != super::json(&expected.head)?.as_bytes()
        || text(row, 3, 64)? != expected.digest.as_bytes()
    {
        return Err("index schema or head mismatch".into());
    }
    if db(rows.next())?.is_some() {
        return Err("unexpected index metadata row".into());
    }
    Ok(())
}

pub(super) struct PayloadBudget {
    remaining: usize,
}
impl Default for PayloadBudget {
    fn default() -> Self {
        Self {
            remaining: memory::SNAPSHOT,
        }
    }
}
impl PayloadBudget {
    pub fn decode<T: DeserializeOwned>(
        &mut self,
        row: &Row<'_>,
        column: usize,
    ) -> Result<T, String> {
        let bytes = text(row, column, self.remaining.min(memory::SNAPSHOT))?;
        self.remaining -= bytes.len();
        serde_json::from_slice(bytes).map_err(|e| format!("derived index JSON: {e}"))
    }
}

pub(super) fn row_limit(expected: usize) -> Result<i64, String> {
    expected
        .checked_add(1)
        .and_then(|n| i64::try_from(n).ok())
        .ok_or_else(|| "derived index row limit overflow".into())
}

pub(super) fn rows<T: DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    expected: usize,
    budget: &mut PayloadBudget,
) -> Result<Vec<T>, String> {
    let mut statement = db(conn.prepare(sql))?;
    let mut rows = db(statement.query([row_limit(expected)?]))?;
    let mut values = Vec::new();
    while let Some(row) = db(rows.next())? {
        if values.len() >= expected {
            return Err("unexpected index row beyond ledger count".into());
        }
        values.push(budget.decode(row, 0)?);
    }
    if values.len() != expected {
        return Err("index row count differs from ledger".into());
    }
    Ok(values)
}
