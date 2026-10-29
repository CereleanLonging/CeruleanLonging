"""Test-build checkpoints coordinate real Windows I/O and owned process faults."""
from pathlib import Path
import ctypes
from ctypes import wintypes
import hashlib,json,os,subprocess,time,uuid
from datetime import datetime,timezone

ROOT=Path(__file__).resolve().parent.parent
RUN=ROOT/'reports/reader-checkpoints-125';RUN.mkdir()
NEW=ROOT/'reports/update-reader-123-gates/reader3-runtime/cerulean-runtime.exe'
calls=[]
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def write(p,v):
    with Path(p).open('x',encoding='utf-8') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
def snap(p):return {q.relative_to(p).as_posix():sha(q) for q in p.rglob('*') if q.is_file()}
def raw(binary,args,env=None,success=True,payload=None):
    p=subprocess.Popen([str(binary),*map(str,args)],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=subprocess.CREATE_NO_WINDOW)
    try:o,e=p.communicate(payload,timeout=30)
    except subprocess.TimeoutExpired:
        write(RUN/f'unresolved-{p.pid}.json',dict(pid=p.pid,args=list(map(str,args)),state='no-retry'));raise
    r=dict(pid=p.pid,binary=str(binary),args=list(map(str,args)),exit=p.returncode,stdoutEOF=True,stderrEOF=True,stdout=o.decode('utf-8'),stderr=e.decode('utf-8'))
    calls.append(r);write(RUN/f'call-{len(calls):03}.json',r)
    assert (p.returncode==0)==success,r
    return r
def cli(data,args,success=True):
    r=raw(NEW,['--workbench','--data-dir',data,*args],success=success)
    if not success:return r
    v=json.loads(r['stdout']);assert v['ok'] is True,v;return v['result']
def decode(p):
    b=p.read_bytes();return b.decode('utf-16') if b[:2] in (b'\xff\xfe',b'\xfe\xff') else b.decode('utf-8-sig')
items=[json.loads(l) for l in decode(ROOT/'reports/reader-fault-125-test-build.jsonl').splitlines() if l.startswith('{')]
assert items[-1]=={'reason':'build-finished','success':True}
artifacts=[i for i in items if i.get('reason')=='compiler-artifact' and i['target']['name']=='cerulean_core' and i['profile']['test'] and i.get('executable')]
assert len(artifacts)==1
TEST=Path(artifacts[0]['executable']);test_hash=sha(TEST)
assert sha(NEW)==json.loads((ROOT/'reports/update-reader-123-gates/result.json').read_text(encoding='utf-8'))['newRuntimeSha256']
kernel=ctypes.WinDLL('kernel32',use_last_error=True)
kernel.CreateFileW.argtypes=[wintypes.LPCWSTR,wintypes.DWORD,wintypes.DWORD,ctypes.c_void_p,wintypes.DWORD,wintypes.DWORD,wintypes.HANDLE];kernel.CreateFileW.restype=wintypes.HANDLE
kernel.LockFile.argtypes=[wintypes.HANDLE,wintypes.DWORD,wintypes.DWORD,wintypes.DWORD,wintypes.DWORD];kernel.LockFile.restype=wintypes.BOOL
kernel.UnlockFile.argtypes=kernel.LockFile.argtypes;kernel.UnlockFile.restype=wintypes.BOOL
kernel.CloseHandle.argtypes=[wintypes.HANDLE];kernel.CloseHandle.restype=wintypes.BOOL
cases=[]
for kind,phase in [('interrupt',p) for p in ('before-create','pending-created','pending-synced','marker-published')]+[('io',p) for p in ('pending-created','pending-synced')]:
    group=RUN/f'{kind}-{phase}'
    env=os.environ.copy();env['CERULEAN_READER_FIXTURE_DIR']=str(group)
    raw(TEST,['--exact','reader_generation::tests::export_reader_cli_fixtures','--ignored'],env=env)
    for fixture in json.loads((group/'cases.json').read_text(encoding='utf-8')):
        data=Path(fixture['data']);backup=Path(fixture['backup']);case=data.parent
        before=snap(data);backup_before=snap(backup);write(case/'before.json',dict(data=before,backup=backup_before))
        args=['--from',backup,'--manifest-digest',fixture['manifestDigest'],'--hold-digest',fixture['holdDigest']]
        review=cli(data,['data-format','review-reader',*args])
        config=dict(kind=kind,phase=phase,data=str(data),backup=str(backup),manifestDigest=fixture['manifestDigest'],holdDigest=fixture['holdDigest'],
                    reviewDigest=review['digest'],checkpoint=str(case/'checkpoint.json'),token=uuid.uuid4().hex)
        if kind=='io':config.update(resume=str(case/'resume-token'),outcome=str(case/'io-error.txt'))
        config_file=case/'config.json';write(config_file,config)
        child_env=os.environ.copy();child_env['CERULEAN_READER_FAULT_CONFIG']=str(config_file)
        p=subprocess.Popen([str(TEST),'--exact','reader_generation::tests::reader_fault_child','--ignored','--nocapture'],env=child_env,
                           stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=subprocess.CREATE_NO_WINDOW)
        write(case/'launch.json',dict(pid=p.pid,testBinary=str(TEST),sha256=test_hash,kind=kind,phase=phase,testOnlyCheckpoints=True,configSha256=sha(config_file)))
        deadline=time.monotonic()+25;receipt=None
        while time.monotonic()<deadline:
            if Path(config['checkpoint']).is_file():
                try:receipt=json.loads(Path(config['checkpoint']).read_text(encoding='utf-8'))
                except json.JSONDecodeError:pass
            if receipt or p.poll() is not None:break
            time.sleep(.02)
        if receipt!={'phase':phase,'pid':p.pid,'token':config['token'],'testOnlyCheckpoint':True} or p.poll() is not None:
            write(case/'unresolved.json',dict(pid=p.pid,checkpoint=receipt,exit=p.poll(),automaticRetry=False));raise RuntimeError('Original checkpoint not confirmed')
        refused=cli(data,['upgrade','release','--hold-digest',fixture['holdDigest']],success=False)
        assert (data/'upgrade-hold.json').is_file() and p.poll() is None
        lock=None;byte_locked=False
        if kind=='interrupt':p.kill()
        else:
            pending=list((data/'events').glob('.format-migration-reader-*.pending'));assert len(pending)==1
            lock=kernel.CreateFileW(str(pending[0]),0xC0000000,3,None,3,0x80,None)
            assert lock!=ctypes.c_void_p(-1).value,ctypes.get_last_error()
            if phase=='pending-created':
                assert kernel.LockFile(lock,0,0,1048576,0),ctypes.get_last_error();byte_locked=True
            with Path(config['resume']).open('x',encoding='utf-8') as f:f.write(config['token'])
        try:
            try:out,err=p.communicate(timeout=25)
            except subprocess.TimeoutExpired:
                write(case/'unresolved-exit.json',dict(pid=p.pid,state='original-child-awaiting-exit',automaticRetry=False));raise
        finally:
            if lock is not None:
                if byte_locked:assert kernel.UnlockFile(lock,0,0,1048576,0)
                assert kernel.CloseHandle(lock)
        (case/'child.stdout').write_bytes(out);(case/'child.stderr').write_bytes(err)
        failure=None
        if kind=='io':
            assert p.returncode==0,(out,err)
            failure=Path(config['outcome']).read_text(encoding='utf-8')
            assert f'(os error {33 if phase=="pending-created" else 32})' in failure,failure
        else:assert p.returncode!=0
        write(case/'child-exit.json',dict(pid=p.pid,exit=p.returncode,stdoutEOF=True,stderrEOF=True,kind=kind,phase=phase,ioError=failure))
        after=snap(data);assert all(after.get(n)==h for n,h in before.items()) and snap(backup)==backup_before
        extra=set(after)-set(before)
        is_pending=phase in ('pending-created','pending-synced')
        if is_pending:
            assert len(extra)==1 and next(iter(extra)).startswith('events/.format-migration-reader-')
            pending_path=data/next(iter(extra));assert (pending_path.stat().st_size==0)==(phase=='pending-created')
            cli(data,['data-format','status'],success=False)
            cli(data,['upgrade','release','--hold-digest',fixture['holdDigest']],success=False)
            assert snap(data)==after
            # Explicit recovery, preserving the original partial tree; never delete a pending marker.
            recovery=cli(data,['recovery','review',*args])
            cli(data,['recovery','prepare',*args,'--review-digest',recovery['digest']])
            restored=cli(data,['recovery','apply','--review-digest',recovery['digest']])
            assert restored['restorationComplete'] and snap(data)==before
            cli(data,['recovery','finish','--review-digest',recovery['digest']])
        else:
            assert extra==({'events/00000000.reader3.json'} if phase=='marker-published' else set())
        status=cli(data,['data-format','status'])
        expected_reader=3 if phase=='marker-published' else fixture['schema']
        assert status['minimumReaderGeneration']==expected_reader and status['dataSchema']==fixture['schema']
        cli(data,['upgrade','release','--hold-digest',fixture['holdDigest']])
        snapshot_reply=json.loads(raw(NEW,['--stdio','--data-dir',data],payload=b'{"id":"checkpoint125","protocolVersion":1,"command":{"type":"snapshot"}}\n')['stdout'])
        assert snapshot_reply['error'] is None and snapshot_reply['result']['product']=='CeruleanLonging'
        case_result=dict(schema=fixture['schema'],kind=kind,phase=phase,pid=p.pid,exit=p.returncode,
                         originalFilesPreserved=True,backupBytesUnchanged=True,partialDataRejected=is_pending,
                         explicitRecovery=is_pending,expectedMinimumReader=expected_reader,holdExplicitlyReleased=True,
                         currentReaderOpened=True,automaticPromotionRetry=False,ioError=failure)
        write(case/'result.json',case_result);cases.append(case_result)
write(RUN/'result.json',dict(at=datetime.now(timezone.utc).isoformat(),status='passed',cases=cases,calls=len(calls),testBinarySha256=test_hash,
      productionProbeRuntimeSha256=sha(NEW),testOnlyCheckpoints=True,realWindowsIo=True,realModelCalls=0,userApplicationActions=0,
      limits=['Checkpoint timing uses a library test binary. Production images have no fault configuration reader.',
              'Actual OS write/share-delete errors and termination of owned original process handles, not power-loss, disk-full or whole-OS failure.',
              'Initial held backups are controlled fixtures. All diagnosis/recovery/readability commands use actual uninstrumented core.',
              'Core/schema1 recovery from original still-held backup; previous-hold schema2 transaction-specific interrupted recovery remains a separate obligation.']))
print(json.dumps(dict(status='passed',cases=len(cases),calls=len(calls))))
