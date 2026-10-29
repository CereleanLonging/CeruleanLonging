"""117 continue only the original failed transaction, retaining first driver failure."""
from pathlib import Path
from hashlib import sha256
from datetime import datetime, timezone, timedelta
import ctypes
import json
import subprocess
import tempfile
import traceback

root = Path(__file__).resolve().parent.parent
original_run = root / 'reports/recovery-share-20261013-nk5dmxx4'
run = Path(tempfile.mkdtemp(prefix='recovery-share-resume-20261013-', dir=root / 'reports'))
exe = root / 'reports/p6-windows-package-018-20261012/cerulean-runtime.exe'
data = original_run / 'data'
calls = []
processes = []
pending = []
checks = {}

def now():
    return datetime.now(timezone(timedelta(hours=8))).isoformat()

def save(path, value):
    assert not path.exists(), str(path)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')

def digest(path):
    resolved = str(path.resolve())
    extended = resolved if resolved.startswith('\\\\?\\') else '\\\\?\\' + resolved
    return sha256(Path(extended).read_bytes()).hexdigest()

def fingerprint(folder):
    return {p.relative_to(folder).as_posix(): {'sha256': digest(p), 'bytes': p.stat().st_size}
            for p in folder.rglob('*') if p.is_file()}

def load(path):
    return json.loads(path.read_text(encoding='utf-8-sig'))

def command(args, payload=None, success=True):
    stem = 'call-%03d' % (len(calls) + 1)
    argv = [str(exe), *map(str, args)]
    save(run / (stem + '.request.json'), {'at': now(), 'args': argv, 'payload': payload, 'expectedSuccess': success})
    p = subprocess.Popen(argv, cwd=run, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW)
    processes.append(p)
    try:
        out, err = p.communicate(None if payload is None else (json.dumps(payload, ensure_ascii=False) + '\n').encode('utf-8'), timeout=45)
    except subprocess.TimeoutExpired:
        pending.append({'pid': p.pid, 'args': argv, 'state': 'original-process-pending-no-replay'})
        save(run / (stem + '.pending.json'), pending[-1])
        raise RuntimeError('Original process pending; no replay')
    (run / (stem + '.stdout')).write_bytes(out)
    (run / (stem + '.stderr')).write_bytes(err)
    receipt = {'at': now(), 'pid': p.pid, 'hasExited': p.poll() is not None,
               'exit': p.returncode, 'args': argv, 'expectedSuccess': success, 'evidence': stem}
    calls.append(receipt)
    save(run / (stem + '.receipt.json'), receipt)
    assert (p.returncode == 0) == success, (argv, p.returncode, out, err)
    return json.loads(out) if success else {'exit': p.returncode, 'stdout': out.decode('utf-8'), 'stderr': err.decode('utf-8')}

def cli(*args, success=True):
    return command(['--workbench', '--data-dir', data, *args], success=success)

def stdio(operation, success=True):
    return command(['--stdio', '--data-dir', data], {'id': 'acceptance-117-resume', 'protocolVersion': 1, 'command': operation}, success)

try:
    failure_hash = digest(original_run / 'result.json')
    baseline = load(original_run / 'session.json')
    assert digest(exe) == baseline['artifacts']['runtime']
    assert digest(exe.parent / 'manifest.json') == baseline['artifacts']['manifest']
    assert load(original_run / 'result.json')['status'] == 'failed'
    assert load(original_run / 'failure-handle-release.json')['fixtureHandleClosed']
    failure = load(original_run / 'call-010.receipt.json')
    assert failure['hasExited'] and failure['exit'] == 3
    assert '(os error 5)' in (original_run / 'call-010.stderr').read_text(encoding='utf-8')
    recovery = load(original_run / 'call-008.stdout')['result']
    tx = Path(recovery['transaction'])
    hold = load(original_run / 'call-006.stdout')['upgradeHold']['holdDigest']
    originals = load(original_run / 'original-manifests.json')
    original = originals['backup']
    faulted = originals['faulted']
    plans = load(original_run / 'call-003.stdout')['result']['plans']
    assert fingerprint(data) == faulted
    before = {'data': fingerprint(data), 'transaction': fingerprint(tx)}
    intent = digest(tx / 'move-intent.json')
    state = cli('recovery', 'status')['result']
    assert state['originalState'] == 'original' and state['moveIntent']
    assert state['stagedBackup'] and not state['preservedOriginal'] and not state['restorationComplete']
    diagnosis = cli('recovery', 'diagnose')['result']
    inspection = cli('recovery', 'inspect', '--review-digest', recovery['digest'])['result']
    cli('recovery', 'abandon', '--review-digest', recovery['digest'], success=False)
    cli('recovery', 'finish', '--review-digest', recovery['digest'], success=False)
    cli('upgrade', 'release', '--hold-digest', hold, success=False)
    stdio({'type': 'snapshot'}, success=False)
    assert before == {'data': fingerprint(data), 'transaction': fingerprint(tx)}
    save(run / 'original-failure-audit.json', {'at': now(), 'originalFailureSha256': failure_hash,
        'originalFailureExit': failure['exit'], 'windowsError': 5, 'status': state,
        'diagnosis': diagnosis, 'inspection': inspection, 'manifests': before,
        'moveIntentSha256': intent, 'queriesAndRejectedActionsUnchanged': True,
        'decision': 'No active original command, original data unmoved, valid staged backup. Fixture handle released. Explicit same-transaction continuation; no new Prepare.'})
    applied = cli('recovery', 'apply', '--review-digest', recovery['digest'])['result']
    assert applied['restorationComplete'] and applied['preservedOriginal'] and applied['startupBlocked']
    assert fingerprint(data) == original and fingerprint(tx / 'preserved') == faulted
    assert digest(tx / 'move-intent.json') == intent
    archive = Path(cli('recovery', 'finish', '--review-digest', recovery['digest'])['result'])
    assert not tx.exists() and fingerprint(archive / 'preserved') == faulted
    assert fingerprint(data) == original and (data / 'upgrade-hold.json').exists()
    before_inspect = {'data': fingerprint(data), 'archive': fingerprint(archive)}
    inspected = cli('recovery', 'inspect', '--review-digest', recovery['digest'])['result']
    assert len(inspected['archives']) == 1 and inspected['archives'][0]['verified']
    assert not inspected['currentDataVerified'] and not inspected['automaticReplay']
    assert before_inspect == {'data': fingerprint(data), 'archive': fingerprint(archive)}
    assert digest(archive / 'move-intent.json') == intent
    save(run / 'archive-verified.json', {'at': now(), 'inspection': inspected,
        'manifests': before_inspect, 'moveIntentSha256': intent})
    cli('upgrade', 'release', '--hold-digest', hold)
    assert stdio({'type': 'snapshot'})['result']['plans'] == plans
    assert cli('recovery', 'inspect', '--review-digest', recovery['digest'])['result']['archives'][0]['verified']
    assert digest(original_run / 'result.json') == failure_hash
    assert digest(exe) == baseline['artifacts']['runtime']
    assert digest(exe.parent / 'manifest.json') == baseline['artifacts']['manifest']
    # Verify final service endpoint absent without starting it; failure is expected.
    stopped = cli('status', success=False)
    assert 'connectionFailed' in stopped['stderr'], stopped
    checks = {'originalDriverFailureRetained': True, 'originalApplyFailureExit3': True,
        'windowsDenyDeleteFaultObservedAsError5': True, 'moveIntentPersistentAndReused': True,
        'abandonFinishReleaseStartupRejected': True, 'queriesAndRejectionsReadOnly': True,
        'explicitSameTransactionContinuation': True, 'prepareNotRepeated': True,
        'productPreservedOriginalExact': True, 'backupRestoredExact': True,
        'archiveVerifiedReadOnly': True, 'holdReleasedExplicitly': True,
        'approvedPlansRecovered': True, 'residentEndpointAbsent': True,
        'frozenArtifactHashesUnchangedNotAcceptanceByItself': True}
    result = {'status': 'passed', 'checks': checks, 'archive': str(archive), 'reviewDigest': recovery['digest'],
        'originalFailure': str(original_run / 'result.json'), 'originalFailureSha256': failure_hash,
        'baselineArtifacts': baseline['artifacts'], 'driverSha256': digest(Path(__file__))}
except Exception as exc:
    result = {'status': 'failed', 'checks': checks, 'error': str(exc), 'traceback': traceback.format_exc()}
finally:
    result.update({'at': now(), 'run': str(run), 'calls': calls, 'pending': pending,
        'allDirectChildrenExited': all(p.poll() is not None for p in processes),
        'cleanup': 'passed' if not pending and checks.get('residentEndpointAbsent') else 'needs-original-state-audit',
        'productModified': False, 'modelCalls': 0, 'userDesktopActions': 0,
        'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False,
        'fullP6Accepted': False, 'releaseAccepted': False,
        'guiAccepted': False, 'installedEntryAccepted': False,
        'diskFullTested': False, 'syncFailureTested': False, 'powerLossTested': False})
    save(run / 'result.json', result)
    print(json.dumps(result, ensure_ascii=True))
if result['status'] != 'passed':
    raise SystemExit(1)
