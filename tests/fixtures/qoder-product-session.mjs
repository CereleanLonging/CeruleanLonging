import { DatabaseSync } from 'node:sqlite';
import { pathToFileURL } from 'node:url';

export function readSession(path, session) {
  if (!/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i.test(session)) {
    throw new Error('Invalid session ID');
  }
  const db = new DatabaseSync(path, { readOnly: true, timeout: 2000 });
  try {
    const rows = db.prepare(
      'SELECT cwd, model, workspace_id, execution_kind FROM chat_sessions WHERE session_id=?'
    ).all(session);
    if (rows.length !== 1 || Object.values(rows[0]).some(v => typeof v !== 'string')) {
      throw new Error('Selected session metadata unavailable');
    }
    return rows[0];
  } finally {
    db.close();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    if (process.argv.length !== 4) throw new Error('Invalid arguments');
    process.stdout.write(JSON.stringify(readSession(process.argv[2], process.argv[3])));
  } catch {
    // Do not expose SQLite contents, path values, or unrelated chat records.
    process.stderr.write('Read-only session metadata unavailable\n');
    process.exitCode = 1;
  }
}
