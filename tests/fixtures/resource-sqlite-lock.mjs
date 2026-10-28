// Test-only SQLite file lock. It never opens a workbench data directory.
import { DatabaseSync } from 'node:sqlite';

const [mode, file] = process.argv.slice(2);
if (!['init', 'hold', 'probe'].includes(mode) || !file) {
  throw new Error('mode and isolated SQLite path required');
}

const db = new DatabaseSync(file, { timeout: 0 });
let transactionOpen = false;
try {
  db.exec('PRAGMA busy_timeout = 0');
  if (mode === 'init') {
    const journal = db.prepare('PRAGMA journal_mode = DELETE').get().journal_mode;
    if (journal.toLowerCase() !== 'delete') throw new Error('DELETE journal required');
    db.exec('CREATE TABLE lock_probe (owner TEXT NOT NULL)');
    process.stdout.write('INITIALIZED\n');
  } else if (mode === 'probe') {
    try {
      db.exec('BEGIN IMMEDIATE');
      transactionOpen = true;
      db.exec('ROLLBACK');
      transactionOpen = false;
      process.stdout.write('FREE\n');
    } catch (error) {
      const busy = error?.code === 'SQLITE_BUSY' ||
        (error?.code === 'ERR_SQLITE_ERROR' && error?.errcode === 5 &&
          error?.errstr === 'database is locked');
      if (!busy) throw error;
      process.stdout.write('BUSY\n');
    }
  } else {
    db.exec('BEGIN EXCLUSIVE');
    transactionOpen = true;
    db.prepare('INSERT INTO lock_probe (owner) VALUES (?)').run('controlled-worker-a');
    process.stdout.write('READY\n');
    let command = '';
    for await (const chunk of process.stdin) {
      command += chunk.toString('utf8');
      if (command.includes('\n')) break;
    }
    if (command.trim() !== 'release') throw new Error('holder released without explicit signal');
    db.exec('ROLLBACK');
    transactionOpen = false;
    process.stdout.write('RELEASED\n');
  }
} finally {
  if (transactionOpen) db.exec('ROLLBACK');
  db.close();
}
