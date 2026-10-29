"""Terminate the frozen shipping migrator after publication, before stdout ACK.

A full anonymous output pipe holds the result write. No test hook is compiled
into this process; observation and termination use its original Popen handle.
"""
from pathlib import Path
from hashlib import sha256
import json
import os
import subprocess
import tempfile
import time

root = Path(__file__).resolve().parent.parent
run = Path(tempfile.mkdtemp(prefix='migration-lost-ack-', dir=root / 'reports'))
data = run / 'data'; backup = run / 'backup'
calls = []; pending = []; active_core = False

def digest(path):
    return sha256(path.read_bytes()).hexdigest()

def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf8')

def fingerprint(folder):
    return {p.relative_to(folder).as_posix(): digest(p) for p in folder.rglob('*') if p.is_file()}

def command(args, operation=None, success=True):
    stem = f'call-{len(calls)+1:03d}'
    p = subprocess.Popen([str(exe), *map(str, args)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, cwd=run, creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        out, err = p.communicate(None if operation is None else (json.dumps(operation, ensure_ascii=False)+'\n').encode('utf8'), timeout=30)
    except subprocess.TimeoutExpired:
        pending.append({'pid': p.pid, 'args': list(map(str, args)), 'state': 'observation-pending-do-not-restart'})
        write(run / 'pending-processes.json', pending)
        raise RuntimeError('Original process observation is pending')
    (run / (stem+'.stdout')).write_bytes(out); (run / (stem+'.stderr')).write_bytes(err)
    calls.append({'pid': p.pid, 'args': list(map(str, args)), 'exit': p.returncode, 'evidence': stem})
    assert (p.returncode == 0) == success, (args, out, err)
    return json.loads(out) if success else {'exit': p.returncode}

def cli(*args, success=True):
    return command(['--workbench', '--data-dir', data, *args], success=success)

def stdio(operation):
    return command(['--stdio', '--data-dir', data], {'id': 'lost-ack', 'protocolVersion': 1, 'command': operation})

try:
    package = root / 'reports/p6-windows-package-015-20261012'
    assert digest(package/'manifest.json') == '7266810b49283529c8b4952891138b97ad362d575d1bbb8a8754e6173c14ef32'
    manifest = json.loads((package/'manifest.json').read_text(encoding='utf-8-sig'))
    exe = package/'cerulean-runtime.exe'
    exe_hash = digest(exe)
    assert exe_hash == next(x['sha256'] for x in manifest['artifacts'] if x['path'] == exe.name)
    assert command(['--build-info'])['version'] == '0.1.5'
    plan = stdio({'type': 'createPlan', 'title': '已迁移但响应丢失', 'tasks': ['不得重做迁移']})['result']['plans'][0]
    plans = stdio({'type': 'approvePlan', 'id': plan['id'], 'digest': plan['digest']})['result']['plans']
    active_core = True; cli('service', 'start')
    review = cli('upgrade', 'review')['upgradeReview']; assert review['ready']
    hold = cli('upgrade', 'prepare', '--review-digest', review['digest'])['upgradeHold']['holdDigest']; active_core = False
    backup_digest = cli('upgrade', 'backup', '--hold-digest', hold, '--to', backup)['result']['manifestDigest']
    before = fingerprint(data); assert before == fingerprint(backup/'payload')
    args = ['--from', backup, '--manifest-digest', backup_digest, '--hold-digest', hold]
    migration = cli('data-format', 'review', *args)['result']
    # CPython's Windows os.pipe creates a 4096-byte binary CRT pipe. Fill it
    # before launch; no reader drains it until the exact child has exited.
    read_fd, write_fd = os.pipe()
    filler = b'P' * 4096
    assert os.write(write_fd, filler) == len(filler)
    err_file = (run/'migrator.stderr').open('wb')
    argv = [str(exe), '--workbench', '--data-dir', str(data), 'data-format', 'migrate',
            *map(str, args), '--review-digest', migration['digest']]
    p = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=write_fd, stderr=err_file,
                         cwd=run, creationflags=subprocess.CREATE_NO_WINDOW)
    os.close(write_fd)
    launch = {'pid': p.pid, 'executable': str(exe), 'sha256': exe_hash, 'args': argv[1:],
              'instrumented': False, 'outputPipePrefillBytes': len(filler)}
    write(run/'launch.json', launch)
    marker = data/'events/00000000.json'
    deadline = time.monotonic()+30
    while not marker.exists() and p.poll() is None and time.monotonic()<deadline:
        time.sleep(0.01)
    if not marker.exists() or p.poll() is not None:
        if p.poll() is None:
            pending.append({**launch, 'state': 'publication-observation-pending-original-process-preserved'})
            write(run/'pending-processes.json', pending)
        raise RuntimeError('Shipped migrator not observed alive after publication; no retry')
    observed = cli('data-format', 'status')['result']
    assert observed['dataSchema'] == 2 and observed['sourceBackupDigest'] == backup_digest
    assert p.poll() is None and (data/'upgrade-hold.json').exists()
    write(run/'published-before-termination.json', {'pid': p.pid, 'alive': True, 'status': observed,
                                                 'holdDigest': digest(data/'upgrade-hold.json')})
    p.kill()
    try:
        exit_code = p.wait(timeout=10)
    except subprocess.TimeoutExpired:
        pending.append({**launch, 'state': 'termination-requested-wait-for-original-exit'})
        write(run/'pending-processes.json', pending)
        raise RuntimeError('Original migrator exit not yet observed')
    err_file.close()
    output = b''
    while True:
        chunk = os.read(read_fd, 8192)
        if not chunk:
            break
        output += chunk
    os.close(read_fd)
    assert output.startswith(filler)
    received = output[len(filler):]
    (run/'migrator.stdout').write_bytes(received)
    assert exit_code != 0 and received == b'', 'Expected no migration ACK bytes'
    write(run/'terminated.json', {**launch, 'hasExited': True, 'exitCode': exit_code, 'ackBytes': len(received)})
    after = fingerprint(data)
    assert set(after)-set(before) == {'events/00000000.json'}
    assert all(after[name] == value for name, value in before.items())
    assert cli('data-format', 'status')['result'] == observed
    cli('data-format', 'migrate', *args, '--review-digest', migration['digest'], success=False)
    assert fingerprint(data) == after and (data/'upgrade-hold.json').exists()
    # Explicitly accept the observed published result, then release the original
    # hold once. No second successful migration invocation is made.
    cli('upgrade', 'release', '--hold-digest', hold)
    assert stdio({'type': 'snapshot'})['result']['plans'] == plans
    result = {'status': 'passed', 'run': str(run), 'pid': p.pid, 'exitCode': exit_code, 'hasExited': True,
              'shippedRuntimeSha256': exe_hash, 'instrumentedMigrationBuild': False,
              'publicationObservedBeforeTermination': True, 'migrationAckBytes': len(received),
              'originalFilesUnchanged': True, 'holdKeptUntilExplicitRelease': True,
              'repeatMigrationRejectedWithoutMutation': True, 'approvedPlansPreserved': True,
              'modelCalls': 0, 'userDesktopActions': 0, 'powerLossTested': False, 'diskFullTested': False,
              'backupDigest': backup_digest, 'observedStatus': observed, 'calls': calls}
except Exception as exc:
    result = {'status': 'failed', 'run': str(run), 'error': str(exc), 'pending': pending, 'calls': calls}
finally:
    cleanup = 'passed'
    if active_core and not pending:
        try:
            cli('service', 'stop')
        except Exception as exc:
            cleanup = str(exc)
    if pending:
        cleanup = 'original-process-preserved'
    result['cleanup'] = cleanup
    write(run/'result.json', result)
    print(json.dumps(result, ensure_ascii=False))
if result['status'] != 'passed' or cleanup != 'passed':
    raise SystemExit(1)
