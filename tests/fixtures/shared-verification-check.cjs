// Owned, pinned independent verifier. The signal directory is an approved arg;
// no inherited environment, model call, network access, or shell invocation.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
assert.ok(['alpha', 'beta'].includes(fs.readFileSync('result.txt', 'utf8')));
assert.equal(fs.existsSync('bundle.txt'), false);
if (fs.existsSync('a.txt')) {
  const control = process.argv[2];
  assert.ok(path.isAbsolute(control));
  fs.writeFileSync(path.join(control, 'entered'), 'one verifier', { flag: 'wx' });
  const deadline = performance.now() + 25000;
  const signal = new Int32Array(new SharedArrayBuffer(4));
  while (!fs.existsSync(path.join(control, 'release'))) {
    assert.ok(performance.now() < deadline, 'controlled verifier gate expired');
    Atomics.wait(signal, 0, 0, 10);
  }
}
