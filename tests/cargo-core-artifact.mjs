import assert from 'node:assert/strict';
import { basename, dirname, resolve } from 'node:path';

export function coreLibraryFromMessages(messages, deps) {
  const records = messages.replace(/^\uFEFF/, '').split(/\r?\n/).filter(line => line.trim()).map(line => JSON.parse(line));
  assert.equal(records.at(-1)?.reason, 'build-finished', 'Completed Cargo artifact stream required');
  assert.equal(records.at(-1).success, true, 'Successful Cargo build required');
  const libraries = records.filter(record => record.reason === 'compiler-artifact'
    && record.target.name === 'cerulean_core' && record.target.kind.includes('lib') && !record.profile.test)
    .flatMap(record => record.filenames.filter(file => basename(file).startsWith('libcerulean_core-') && file.endsWith('.rlib')));
  const unique = [...new Set(libraries.map(file => resolve(file)))];
  assert.equal(unique.length, 1, 'Exactly one core library from this Cargo build required');
  assert.equal(dirname(unique[0]), resolve(deps), 'Core artifact must belong to the requested dependency directory');
  return basename(unique[0]);
}
