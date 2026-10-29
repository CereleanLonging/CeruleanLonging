"""Lose the real development CLI finish/abandon response after archival.
Full stdout pipe, original child handle, no product test hooks or replay.
"""
from pathlib import Path
from hashlib import sha256
import json
import os
import subprocess
import tempfile
import time
import uuid
import traceback

root = Path(__file__).resolve().parent.parent
run = Path(tempfile.mkdtemp(prefix='recovery-finish-ack-', dir=root / 'reports'))
calls = []
cases = []
pending = []
active_core = None
worker = None

def digest(path):
    return sha256(path.read_bytes()).hexdigest()

def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf8')

def load_log(path):
    raw = path.read_bytes()
    return raw.decode('utf-16' if raw.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig')

def command(exe, args, payload=None, success=True):
    number = len(calls) + 1
    stem = f'call-{number:03d}'
    p = subprocess.Popen([str(exe), *map(str, args)], cwd=run,
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                         creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        out, err = p.communicate(None if payload is None else (json.dumps(payload, ensure_ascii=False) + '\n').encode('utf8'), timeout=30)
    except subprocess.TimeoutExpired:
        pending.append({'pid': p.pid, 'executable': str(exe), 'args': list(map(str, args)),
                        'state': 'observation-timeout-do-not-restart'})
        write(run / 'pending-processes.json', pending)
        raise RuntimeError('Original process observation pending; no restart')
    (run / (stem + '.stdout')).write_bytes(out)
    (run / (stem + '.stderr')).write_bytes(err)
    calls.append({'pid': p.pid, 'executable': str(exe), 'args': list(map(str, args)),
                  'exit': p.returncode, 'expectedSuccess': success, 'evidence': stem})
    assert (p.returncode == 0) == success, (args, p.returncode, out, err)
    return json.loads(out) if success else {'exit': p.returncode, 'stdout': out.decode('utf8'), 'stderr': err.decode('utf8')}

def cli(data, *args, success=True):
    return command(new, ['--workbench', '--data-dir', data, *args], success=success)

def stdio(exe, data, operation, success=True):
    return command(exe, ['--stdio', '--data-dir', data],
                   {'id': 'recovery-transaction', 'protocolVersion': 1, 'command': operation}, success)

def fingerprint(folder):
    return {p.relative_to(folder).as_posix(): digest(p) for p in folder.rglob('*') if p.is_file()}



try:
    new = root / 'target/debug/cerulean-core.exe'
    old = root / 'reports/p6-windows-package-014-20261012/cerulean-runtime.exe'
    package = json.loads((old.parent / 'manifest.json').read_text(encoding='utf-8-sig'))
    assert digest(old) == next(x['sha256'] for x in package['artifacts'] if x['path'] == old.name)
    for action in ['finish', 'abandon']:
        case = run / action; case.mkdir()
        data = case / 'data'; backup = case / 'backup'
        plan = stdio(new, data, {'type': 'createPlan', 'title': '收尾返回丢失', 'tasks': ['只读核对原事务']})['result']['plans'][0]
        plans = stdio(new, data, {'type': 'approvePlan', 'id': plan['id'], 'digest': plan['digest']})['result']['plans']
        active_core = data; cli(data, 'service', 'start')
        review = cli(data, 'upgrade', 'review')['upgradeReview']; assert review['ready']
        hold = cli(data, 'upgrade', 'prepare', '--review-digest', review['digest'])['upgradeHold']['holdDigest']; active_core = None
        manifest = cli(data, 'upgrade', 'backup', '--hold-digest', hold, '--to', backup)['result']['manifestDigest']
        baseline = fingerprint(data)
        (data / 'events/.format-migration-ack-fixture.pending').write_bytes(b'explicit fixture fault')
        faulted = fingerprint(data)
        args = ['--from', backup, '--manifest-digest', manifest, '--hold-digest', hold]
        review = cli(data, 'recovery', 'review', *args)['result']
        tx = Path(review['transaction'])
        cli(data, 'recovery', 'prepare', *args, '--review-digest', review['digest'])
        if action == 'finish':
            assert cli(data, 'recovery', 'apply', '--review-digest', review['digest'])['result']['restorationComplete']
        assert cli(data, 'recovery', 'inspect', '--review-digest', review['digest'])['result']['archives'] == []
        read_fd, write_fd = os.pipe(); filler = b'P' * 4096
        assert os.write(write_fd, filler) == len(filler)
        err_file = (case / 'worker.stderr').open('wb')
        argv = [str(new), '--workbench', '--data-dir', str(data), 'recovery', action, '--review-digest', review['digest']]
        worker = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=write_fd, stderr=err_file, cwd=run, creationflags=subprocess.CREATE_NO_WINDOW)
        os.close(write_fd)
        launch = {'pid': worker.pid, 'executable': str(new), 'sha256': digest(new), 'args': argv[1:], 'instrumented': False, 'outputPipePrefillBytes': len(filler)}
        write(case / 'launch.json', launch)
        prefix = '.cerulean-recovery-' + ('completed' if action == 'finish' else 'abandoned') + '-' + review['digest'] + '-'
        deadline = time.monotonic() + 30
        while tx.exists() and worker.poll() is None and time.monotonic() < deadline:
            time.sleep(0.01)
        if tx.exists() or worker.poll() is not None:
            raise RuntimeError('Original child was not observed alive after archive publication; no retry')
        observed = None
        while time.monotonic() < deadline:
            candidate = cli(data, 'recovery', 'inspect', '--review-digest', review['digest'])['result']
            if not candidate['guardPresent'] and len(candidate['archives']) == 1 and candidate['archives'][0]['verified']:
                observed = candidate; break
            time.sleep(0.02)
        assert observed is not None and worker.poll() is None
        archived = Path(observed['archives'][0]['archive'])
        assert archived.name.startswith(prefix)
        write(case / 'observed-before-kill.json', {'pid': worker.pid, 'alive': True, 'inspection': observed})
        worker.kill()
        try:
            exit_code = worker.wait(timeout=10)
        except subprocess.TimeoutExpired:
            raise RuntimeError('Original child exit not yet confirmed')
        pid = worker.pid; worker = None; err_file.close()
        output = b''
        while True:
            chunk = os.read(read_fd, 8192)
            if not chunk: break
            output += chunk
        os.close(read_fd)
        assert output.startswith(filler)
        ack = output[len(filler):]; (case / 'worker.stdout').write_bytes(ack)
        assert exit_code != 0 and not ack
        before_query = {'data': fingerprint(data), 'archive': fingerprint(archived)}
        reread = cli(data, 'recovery', 'inspect', '--review-digest', review['digest'])['result']
        assert reread == observed and not reread['currentDataVerified'] and not reread['automaticReplay']
        assert before_query == {'data': fingerprint(data), 'archive': fingerprint(archived)}
        assert (data / 'upgrade-hold.json').exists()
        if action == 'finish':
            assert fingerprint(data) == baseline and fingerprint(archived / 'preserved') == faulted
            cli(data, 'upgrade', 'release', '--hold-digest', hold)
            assert stdio(old, data, {'type': 'snapshot'})['result']['plans'] == plans
            assert cli(data, 'recovery', 'inspect', '--review-digest', review['digest'])['result']['archives'][0]['verified']
        else:
            assert fingerprint(data) == faulted
        item = {'action': action, 'pid': pid, 'exitCode': exit_code, 'hasExited': True, 'ackBytes': len(ack), 'archive': str(archived), 'singleFinalizationAttempt': True, 'inspectionReadOnly': True, 'holdKeptUntilExplicitRelease': True, 'verifiedBeforeAndAfterKill': True}
        cases.append(item); write(case / 'result.json', item)
    result = {'status': 'passed', 'run': str(run), 'cases': cases, 'calls': calls, 'developmentCoreSha256': digest(new), 'realChildTermination': True, 'instrumentedProduct': False, 'shippedReleaseAccepted': False, 'diskFullTested': False, 'powerLossTested': False, 'modelCalls': 0, 'userDesktopActions': 0}
except Exception as exc:
    result = {'status': 'failed', 'run': str(run), 'error': str(exc), 'traceback': traceback.format_exc(), 'cases': cases, 'calls': calls}
finally:
    if worker is not None and worker.poll() is None:
        pending.append({'pid': worker.pid, 'state': 'original-finalization-child-still-live-do-not-restart'})
        write(run / 'pending-processes.json', pending)
    cleanup = 'passed'
    if active_core is not None and not pending:
        try: cli(active_core, 'service', 'stop')
        except Exception as exc: cleanup = str(exc)
    if pending: cleanup = 'original-process-preserved'
    result['cleanup'] = cleanup; result['pending'] = pending
    write(run / 'result.json', result)
    print(json.dumps(result, ensure_ascii=False))
if result['status'] != 'passed' or cleanup != 'passed': raise SystemExit(1)
