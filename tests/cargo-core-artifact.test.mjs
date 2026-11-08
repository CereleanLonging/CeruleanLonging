import assert from 'node:assert/strict';
import { join, resolve } from 'node:path';
import test from 'node:test';
import { coreLibraryFromMessages } from './cargo-core-artifact.mjs';

const deps = resolve('synthetic-cargo/debug/deps');
const library = name => ({ reason: 'compiler-artifact', target: { name: 'cerulean_core', kind: ['lib'] },
  profile: { test: false }, filenames: [join(deps, name)], fresh: true });
const current = library('libcerulean_core-current.rlib');
const finish = { reason: 'build-finished', success: true };
const stream = records => records.map(record => JSON.stringify(record)).join('\r\n') + '\r\n';

test('Cargo output selects the current production library while other variants coexist', () => {
  const records = [
    { ...library('libcerulean_core-test.rlib'), profile: { test: true } },
    { ...current, target: { name: 'cerulean-core', kind: ['bin'] }, filenames: [join(deps, 'cerulean-core.exe')] },
    current, finish,
  ];
  assert.equal(coreLibraryFromMessages('\uFEFF' + stream(records), deps), 'libcerulean_core-current.rlib');
});

test('Repeated reports of the same exact library remain unambiguous', () => {
  assert.equal(coreLibraryFromMessages(stream([current, current, finish]), deps), 'libcerulean_core-current.rlib');
});

test('Different production libraries require an explicit build identity', () => {
  assert.throws(() => coreLibraryFromMessages(stream([current, library('libcerulean_core-other.rlib'), finish]), deps),
    /Exactly one core library/);
});

test('Failed, missing and incomplete Cargo streams cannot select a library', () => {
  assert.throws(() => coreLibraryFromMessages(stream([current]), deps), /Completed Cargo/);
  assert.throws(() => coreLibraryFromMessages(stream([current, { ...finish, success: false }]), deps), /Successful Cargo/);
  assert.throws(() => coreLibraryFromMessages(stream([finish]), deps), /Exactly one core library/);
  assert.throws(() => coreLibraryFromMessages('invalid JSON\n', deps), SyntaxError);
});

test('An artifact from another target directory is rejected', () => {
  assert.throws(() => coreLibraryFromMessages(stream([current, finish]), resolve('other/deps')), /requested dependency directory/);
});
