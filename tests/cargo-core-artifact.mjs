import assert from 'node:assert/strict';
import { basename, dirname, resolve } from 'node:path';

export function coreLibraryFromMessages(messages, deps) {
  const records = messages.replace(/^\uFEFF/, '').split(/\r?\n/).filter(line => line.trim()).map(line => JSON.parse(line));
  assert.equal(records.at(-1)?.reason, 'build-finished', 'Completed Cargo artifact stream required');
  assert.equal(records.at(-1).success, true, 'Successful Cargo build required');
  const artifacts = records.filter(record => record.reason === 'compiler-artifact'
    && record.target.name === 'cerulean_core' && record.target.kind.includes('lib') && !record.profile.test);
  const libraries = artifacts.flatMap(record => record.filenames.filter(file =>
    /^libcerulean_core(?:-[a-zA-Z0-9]+)?\.rlib$/.test(basename(file))));
  const unique = [...new Set(libraries.map(file => resolve(file)))];
  assert.equal(unique.length, 1, 'Exactly one core library from this Cargo build required');
  if (basename(unique[0]) === 'libcerulean_core.rlib') {
    // Cargo reports the primary library at profile level. Its paired metadata
    // carries the exact hashed identity of the library in the dependency folder.
    assert.equal(dirname(unique[0]), dirname(resolve(deps)), 'Core artifact must belong to the requested dependency directory');
    const metadata = artifacts.filter(record => record.filenames.some(file => resolve(file) === unique[0]))
      .flatMap(record => record.filenames.filter(file => /^libcerulean_core-[a-zA-Z0-9]+\.rmeta$/.test(basename(file))));
    const identities = [...new Set(metadata.map(file => resolve(file)))];
    assert.equal(identities.length, 1, 'Exactly one paired core metadata identity required');
    assert.equal(dirname(identities[0]), resolve(deps), 'Core metadata must belong to the requested dependency directory');
    return basename(identities[0]).replace(/\.rmeta$/, '.rlib');
  }
  assert.equal(dirname(unique[0]), resolve(deps), 'Core artifact must belong to the requested dependency directory');
  return basename(unique[0]);
}
