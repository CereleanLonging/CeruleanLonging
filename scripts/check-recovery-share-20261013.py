"""117: frozen 0.1.8 CLI / real Windows deny-delete sharing fault.
Reuse 107 unit-tested mechanism; no hooks, installer, model or user app.
Each invocation has a new evidence directory. Never replay on uncertainty.
"""
from pathlib import Path
from hashlib import sha256
from datetime import datetime, timezone, timedelta
import ctypes
from ctypes import wintypes
import json
import subprocess
import tempfile
import traceback

root = Path(__file__).resolve().parent.parent
run = Path(tempfile.mkdtemp(prefix='recovery-share-20261013-', dir=root / 'reports'))
calls = []
pending = []
active_core = False
handle = None
processes = []
checks = {}
data = run / 'data'
backup = run / 'backup'
exe = root / 'reports/p6-windows-package-018-20261012/cerulean-runtime.exe'
manifest = exe.parent / 'manifest.json'

def now():
    return datetime.now(timezone(timedelta(hours=8))).isoformat()

def save(path, value):
    assert not path.exists(), 'Evidence overwrite prohibited: ' + str(path)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')

def digest(path):
    # pathlib supports Windows extended paths; avoid PS provider long-path reads.
    p = Path('\\\\?\\' + str(path.resolve()))
    return sha256(p.read_bytes()).hexdigest()

def fingerprint(folder):
    return {p.relative_to(folder).as_posix(): {'sha256': digest(p), 'bytes': p.stat().st_size}
            for p in folder.rglob('*') if p.is_file()}

def command(args, payload=None, success=True):
    stem = 'call-%03d' % (len(calls) + 1)
    argv = [str(exe), *map(str, args)]
    save(run / (stem + '.request.json'), {'at': now(), 'argv': argv, 'payload': payload, 'expectedSuccess': success})
    p = subprocess.Popen(argv, cwd=run, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, creationflags=subprocess.CREATE_NO_WINDOW)
    processes.append(p)
    try:
        out, err = p.communicate(None if payload is None else (json.dumps(payload, ensure_ascii=False) + '\n').encode('utf-8'), timeout=45)
    except subprocess.TimeoutExpired:
        pending.append({'pid': p.pid, 'args': argv, 'state': 'original-process-pending-no-replay'})
        save(run / (stem + '.pending.json'), pending[-1])
        raise RuntimeError('Original process pending; do not replay')
    (run / (stem + '.stdout')).write_bytes(out)
    (run / (stem + '.stderr')).write_bytes(err)
    receipt = {'pid': p.pid, 'exit': p.returncode, 'at': now(), 'args': argv,
               'hasExited': p.poll() is not None, 'expectedSuccess': success, 'evidence': stem}
    calls.append(receipt)
    save(run / (stem + '.receipt.json'), receipt)
    assert (p.returncode == 0) == success, (argv, p.returncode, out, err)
    return json.loads(out) if success else {'exit': p.returncode, 'stdout': out.decode('utf-8'), 'stderr': err.decode('utf-8')}

def cli(*args, success=True):
    return command(['--workbench', '--data-dir', data, *args], success=success)

def stdio(operation, success=True):
    return command(['--stdio', '--data-dir', data],
                   {'id': 'acceptance-117', 'protocolVersion': 1, 'command': operation}, success)

kernel = ctypes.WinDLL('kernel32', use_last_error=True)
kernel.CreateFileW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE]
kernel.CreateFileW.restype = wintypes.HANDLE
kernel.CloseHandle.argtypes = [wintypes.HANDLE]
kernel.CloseHandle.restype = wintypes.BOOL

try:
    baseline_artifacts = {'manifest': digest(manifest), 'runtime': digest(exe)}
    assert baseline_artifacts['manifest'] == 'b168842fff78a18556ee3468f4bcb078daa54f4e596de86880337195a9967d9d'
    package = json.loads(manifest.read_text(encoding='utf-8-sig'))
    assert baseline_artifacts['runtime'] == package['coreSha256']
    save(run / 'session.json', {'at': now(), 'artifacts': baseline_artifacts,
        'driverSha256': digest(Path(__file__)), 'runtime': str(exe), 'version': '0.1.8',
        'reuse': '107 real_rename_sharing_failure_retains_intent_then_explicit_apply_continues',
        'scope': 'Frozen runtime CLI entrypoint supplement; not GUI/installed/whole-matrix acceptance'})
    assert command(['--build-info'])['version'] == '0.1.8'
    plan = stdio({'type': 'createPlan', 'title': '117共享冲突独立验收', 'tasks': ['原批准字节与故障目录保留']})['result']['plans'][0]
    plans = stdio({'type': 'approvePlan', 'id': plan['id'], 'digest': plan['digest']})['result']['plans']
    active_core = True
    cli('service', 'start')
    review = cli('upgrade', 'review')['upgradeReview']
    assert review['ready']
    hold = cli('upgrade', 'prepare', '--review-digest', review['digest'])['upgradeHold']['holdDigest']
    active_core = False
    backup_digest = cli('upgrade', 'backup', '--hold-digest', hold, '--to', backup)['result']['manifestDigest']
    original = fingerprint(data)
    assert original == fingerprint(backup / 'payload')
    fault_file = data / 'events/.format-migration-share-117.pending'
    fault_file.write_bytes('Explicit acceptance fixture; not an actual interrupted migration. 中文原证据'.encode('utf-8'))
    faulted = fingerprint(data)
    save(run / 'original-manifests.json', {'backup': original, 'faulted': faulted})
    args = ['--from', backup, '--manifest-digest', backup_digest, '--hold-digest', hold]
    recovery = cli('recovery', 'review', *args)['result']
    tx = Path(recovery['transaction'])
    prepared = cli('recovery', 'prepare', *args, '--review-digest', recovery['digest'])['result']
    assert prepared['originalState'] == 'original' and prepared['stagedBackup']
    assert fingerprint(data) == faulted
    handle = kernel.CreateFileW(str(fault_file), 0x80000000, 3, None, 3, 0x80, None)
    assert handle != ctypes.c_void_p(-1).value, ctypes.get_last_error()
    save(run / 'sharing-handle.json', {'at': now(), 'path': str(fault_file),
        'desiredAccess': 'GENERIC_READ', 'shareMode': 'FILE_SHARE_READ|FILE_SHARE_WRITE; no FILE_SHARE_DELETE',
        'handleObtained': True, 'productInstrumented': False})
    failed = cli('recovery', 'apply', '--review-digest', recovery['digest'], success=False)
    assert '(os error 32)' in failed['stderr'], failed
    assert fingerprint(data) == faulted
    state = cli('recovery', 'status')['result']
    assert state['originalState'] == 'original' and state['moveIntent']
    assert not state['preservedOriginal'] and state['stagedBackup'] and not state['restorationComplete']
    intent = digest(tx / 'move-intent.json')
    before_queries = {'data': fingerprint(data), 'transaction': fingerprint(tx)}
    diagnosis = cli('recovery', 'diagnose')['result']
    inspection = cli('recovery', 'inspect', '--review-digest', recovery['digest'])['result']
    cli('recovery', 'abandon', '--review-digest', recovery['digest'], success=False)
    cli('recovery', 'finish', '--review-digest', recovery['digest'], success=False)
    cli('upgrade', 'release', '--hold-digest', hold, success=False)
    stdio({'type': 'snapshot'}, success=False)
    assert before_queries == {'data': fingerprint(data), 'transaction': fingerprint(tx)}
    save(run / 'fault-observed.json', {'at': now(), 'error': failed, 'status': state,
        'diagnosis': diagnosis, 'inspection': inspection, 'manifests': before_queries,
        'moveIntentSha256': intent, 'readonlyAndRejectedOperationsUnchanged': True})
    assert kernel.CloseHandle(handle), ctypes.get_last_error()
    handle = None
    save(run / 'sharing-handle-released.json', {'at': now(), 'fixtureHandleClosed': True,
        'decision': 'Original failed apply returned; status confirms no move. Explicitly continue same review/transaction once; not automatic replay.'})
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
    save(run / 'archive-verified.json', {'at': now(), 'inspection': inspected, 'manifests': before_inspect,
        'moveIntentSha256': digest(archive / 'move-intent.json')})
    assert digest(archive / 'move-intent.json') == intent
    cli('upgrade', 'release', '--hold-digest', hold)
    assert stdio({'type': 'snapshot'})['result']['plans'] == plans
    assert cli('recovery', 'inspect', '--review-digest', recovery['digest'])['result']['archives'][0]['verified']
    assert baseline_artifacts == {'manifest': digest(manifest), 'runtime': digest(exe)}
    checks = {'windowsShareFailure32': True, 'originalDataUnchangedOnFailure': True,
        'moveIntentPersistentAndReused': True, 'abandonFinishReleaseStartupRejected': True,
        'observationsReadOnly': True, 'explicitSameTransactionContinuation': True,
        'productPreservedDirectoryExact': True, 'restoredBackupExact': True,
        'archiveVerifiedReadOnly': True, 'holdReleaseExplicit': True,
        'approvedPlansRecovered': True, 'frozenArtifactsUnchangedNotAcceptanceByItself': True}
    result = {'status': 'passed', 'checks': checks, 'archive': str(archive), 'reviewDigest': recovery['digest']}
except Exception as exc:
    result = {'status': 'failed', 'error': str(exc), 'traceback': traceback.format_exc(), 'checks': checks}
finally:
    if handle is not None and handle != ctypes.c_void_p(-1).value:
        closed = bool(kernel.CloseHandle(handle))
        save(run / 'failure-handle-release.json', {'at': now(), 'fixtureHandleClosed': closed})
    cleanup = 'passed'
    if active_core and not pending:
        try:
            cli('service', 'stop')
        except Exception as exc:
            cleanup = str(exc)
    if pending:
        cleanup = 'original-process-preserved-no-replay'
    result.update({'at': now(), 'run': str(run), 'calls': calls, 'pending': pending,
        'cleanup': cleanup, 'allDirectChildrenExited': all(p.poll() is not None for p in processes),
        'modelCalls': 0, 'userDesktopActions': 0, 'productModified': False,
        'fullP3Accepted': False, 'fullP4Accepted': False, 'fullP5Accepted': False,
        'fullP6Accepted': False, 'releaseAccepted': False, 'diskFullTested': False,
        'syncFailureTested': False, 'powerLossTested': False})
    save(run / 'result.json', result)
    print(json.dumps(result, ensure_ascii=True))
if result['status'] != 'passed' or cleanup != 'passed':
    raise SystemExit(1)
