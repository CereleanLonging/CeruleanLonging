"""Real development CLI recovery; no shipped GUI/release acceptance implied."""
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
run = Path(tempfile.mkdtemp(prefix='recovery-transaction-', dir=root / 'reports'))
calls = []
cases = []
pending = []
active_core = None

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
    manifest_path = old.parent / 'manifest.json'
    assert digest(manifest_path) == 'a34f0555e843c032a2400b5c5a5c995bb731018c4e60a6c4072003ff13f6b3af'
    package = json.loads(manifest_path.read_text(encoding='utf-8-sig'))
    assert digest(old) == next(x['sha256'] for x in package['artifacts'] if x['path'] == old.name)
    for name in ['apply', 'abandon', 'moved-fixture']:
        case = run / name; case.mkdir()
        data = case / 'data'; backup = case / 'backup'
        created = stdio(new, data, {'type': 'createPlan', 'title': '备份恢复事务', 'tasks': ['保留原批准与故障记录']})
        plan = created['result']['plans'][0]
        approved = stdio(new, data, {'type': 'approvePlan', 'id': plan['id'], 'digest': plan['digest']})['result']['plans']
        active_core = data
        cli(data, 'service', 'start')
        review = cli(data, 'upgrade', 'review')['upgradeReview']; assert review['ready']
        hold = cli(data, 'upgrade', 'prepare', '--review-digest', review['digest'])['upgradeHold']['holdDigest']
        active_core = None
        manifest = cli(data, 'upgrade', 'backup', '--hold-digest', hold, '--to', backup)['result']['manifestDigest']
        baseline = fingerprint(data)
        # Explicit fault fixture, not a claim that a live migration was interrupted.
        (data / 'events/.format-migration-recovery-fixture.pending').write_bytes(b'preserve this failure evidence')
        faulted = fingerprint(data)
        args = ['--from', backup, '--manifest-digest', manifest, '--hold-digest', hold]
        review = cli(data, 'recovery', 'review', *args)['result']
        tx = Path(review['transaction'])
        assert tx.parent.samefile(data.parent) and not tx.exists()
        prepared = cli(data, 'recovery', 'prepare', *args, '--review-digest', review['digest'])['result']
        assert prepared['originalState'] == 'original' and prepared['stagedBackup']
        assert fingerprint(data) == faulted
        cli(data, 'upgrade', 'release', '--hold-digest', hold, success=False)
        cli(data, 'data-format', 'migrate', *args, '--review-digest', '0'*64, success=False)
        cli(data, 'backup', 'restore', '--from', backup, '--manifest-digest', manifest, success=False)
        stdio(new, data, {'type': 'snapshot'}, success=False)
        if name == 'abandon':
            retained = Path(cli(data, 'recovery', 'abandon', '--review-digest', review['digest'])['result'])
            assert retained.is_dir() and not tx.exists() and fingerprint(data) == faulted
            assert (data / 'upgrade-hold.json').exists()
            item = {'name': name, 'originalBytesUnchanged': True, 'holdRetained': True, 'allEvidenceArchived': True}
        else:
            if name == 'moved-fixture':
                # Explicit filesystem interruption fixture, never label as a
                # product process kill. Validate both endpoints before the move.
                assert data.parent.samefile(case)
                assert tx.parent.samefile(case) and case.parent.samefile(run)
                assert not (tx / 'preserved').exists()
                write(tx / 'move-intent.json', review['digest'])
                os.rename(data, tx / 'preserved')
                cli(data, 'service', 'start', success=False)
                cli(data, 'status', success=False)
                assert not data.exists(), 'IPC must not recreate the data directory while recovery is pending'
            applied = cli(data, 'recovery', 'apply', '--review-digest', review['digest'])['result']
            assert applied['restorationComplete'] and applied['startupBlocked'] and applied['preservedOriginal']
            assert fingerprint(data) == baseline
            assert fingerprint(tx / 'preserved') == faulted
            observed = cli(data, 'recovery', 'status')['result']; assert observed == applied
            stdio(new, data, {'type': 'snapshot'}, success=False)
            cli(data, 'upgrade', 'release', '--hold-digest', hold, success=False)
            retained = Path(cli(data, 'recovery', 'finish', '--review-digest', review['digest'])['result'])
            assert retained.is_dir() and not tx.exists()
            assert fingerprint(retained / 'preserved') == faulted
            assert fingerprint(data) == baseline
            cli(data, 'upgrade', 'release', '--hold-digest', hold)
            assert stdio(old, data, {'type': 'snapshot'})['result']['plans'] == approved
            item = {'name': name, 'restoredBytesExact': True, 'faultDirectoryPreservedExact': True,
                    'legacyApprovedPlansRecovered': True, 'holdReleaseExplicit': True,
                    'controllerDidNotMoveOrDeleteDataDirectories': name != 'moved-fixture'}
        cases.append(item); write(case / 'result.json', item)
    result = {'status': 'passed', 'run': str(run), 'cases': cases, 'calls': calls,
              'developmentCoreSha256': digest(new), 'legacyRuntimeSha256': digest(old),
              'fixturePartialMigration': True, 'liveMigrationFault': False,
              'realModelCalls': 0, 'userDesktopActions': 0, 'shippedReleaseAccepted': False}
except Exception as exc:
    result = {'status': 'failed', 'run': str(run), 'error': str(exc), 'traceback': traceback.format_exc(), 'calls': calls, 'pending': pending, 'cases': cases}
finally:
    cleanup = 'passed'
    if active_core is not None and not pending:
        try:
            cli(active_core, 'service', 'stop')
        except Exception as exc:
            cleanup = str(exc)
    if pending: cleanup = 'original-process-preserved'
    result['cleanup'] = cleanup
    write(run / 'result.json', result)
    print(json.dumps(result, ensure_ascii=False))
if result['status'] != 'passed' or cleanup != 'passed': raise SystemExit(1)
