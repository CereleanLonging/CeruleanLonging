import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { mkdtempSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { readSession } from './fixtures/qoder-product-session.mjs';

// Retain the tiny fixture for inspection; never reads a live application DB.
const root = mkdtempSync(join(tmpdir(), 'cerulean-qoder-session-230-'));
const file = join(root, 'fixture.sqlite');
const id = '0fa16906-4f98-4f59-8b4b-9ea6b556982a';
const db = new DatabaseSync(file);
db.exec('CREATE TABLE chat_sessions (session_id TEXT, cwd TEXT, model TEXT, workspace_id TEXT, execution_kind TEXT, private_text TEXT)');
db.prepare('INSERT INTO chat_sessions VALUES (?,?,?,?,?,?)')
  .run(id, 'isolated/project', 'model', 'workspace', 'local', 'unrelated text');
db.close();
const before = readFileSync(file);
assert.deepEqual({ ...readSession(file, id) }, {
  cwd: 'isolated/project', model: 'model', workspace_id: 'workspace', execution_kind: 'local',
});
assert.throws(() => readSession(file, 'invalid'));
assert.throws(() => readSession(file, '76edb3bc-078f-4ce4-bc9b-b6525fde9c51'));
assert.deepEqual(readFileSync(file), before);
const missing = join(root, 'missing.sqlite');
assert.throws(() => readSession(missing, id));
assert.equal(existsSync(missing), false);
console.log('6 session reader checks passed; no live database or credential access');
