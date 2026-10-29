"""Schema2 previous-hold preparation interruption and actual rename-sharing faults."""
from pathlib import Path
from datetime import datetime,timezone
import ctypes
from ctypes import wintypes
import hashlib,json,os,subprocess,time,uuid
ROOT=Path(__file__).resolve().parent.parent
RUN=ROOT/'reports/previous-fault-126-cases'
NEW=ROOT/'reports/update-reader-123-gates/reader3-runtime/cerulean-runtime.exe'
calls=[]
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def write(p,v):
    with Path(p).open('x',encoding='utf-8') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
def snap(p):return {q.relative_to(p).as_posix():sha(q) for q in p.rglob('*') if q.is_file()}
def log(p):
    b=p.read_bytes();return b.decode('utf-16') if b[:2] in (b'\xff\xfe',b'\xfe\xff') else b.decode('utf-8-sig')
def command(binary,args,success=True,payload=None):
    p=subprocess.Popen([str(binary),*map(str,args)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=subprocess.CREATE_NO_WINDOW)
    try:out,err=p.communicate(payload,timeout=30)
    except subprocess.TimeoutExpired:
        write(RUN/f'unresolved-{p.pid}.json',dict(pid=p.pid,args=list(map(str,args)),automaticRetry=False));raise
    record=dict(pid=p.pid,binary=str(binary),args=list(map(str,args)),exit=p.returncode,stdoutEOF=True,stderrEOF=True,stdout=out.decode('utf-8'),stderr=err.decode('utf-8'))
    calls.append(record);write(RUN/f'call-{len(calls):03}.json',record)
    assert (p.returncode==0)==success,record
    return json.loads(out) if success else record
def cli(data,args,success=True):
    r=command(NEW,['--workbench','--data-dir',data,*args],success)
    if success:assert r['ok'] is True;return r['result']
    return r
kernel=ctypes.WinDLL('kernel32',use_last_error=True)
kernel.CreateFileW.argtypes=[wintypes.LPCWSTR,wintypes.DWORD,wintypes.DWORD,ctypes.c_void_p,wintypes.DWORD,wintypes.DWORD,wintypes.HANDLE];kernel.CreateFileW.restype=wintypes.HANDLE
kernel.CloseHandle.argtypes=[wintypes.HANDLE];kernel.CloseHandle.restype=wintypes.BOOL

def main():
    prior=sorted(RUN.glob('call-*.json'))
    assert len(prior)==28 and not (RUN/'resume-original.json').exists() and not (RUN/'result.json').exists(),'Resume only inspected original first sharing failure'
    calls.extend(json.loads(p.read_text(encoding='utf-8')) for p in prior)
    assert calls[-1]['exit']==3 and '(os error 5)' in calls[-1]['stderr']
    write(RUN/'resume-original.json',dict(priorCalls=28,reason='Nested-file deny-delete handle produced directory-move error5, while harness expected32. Original apply failed; fixture finally closed handle. Inspect same transaction before explicit continuation, no repeated preparation.',completedCasesRetained=3))
    assert sha(NEW)==json.loads((ROOT/'reports/update-reader-123-gates/result.json').read_text(encoding='utf-8'))['newRuntimeSha256']
    builds=[json.loads(l) for l in log(ROOT/'reports/previous-fault-126-test-build.jsonl').splitlines() if l.startswith('{')]
    assert builds[-1]=={'reason':'build-finished','success':True}
    tests=[i for i in builds if i.get('reason')=='compiler-artifact' and i['target']['name']=='cerulean_core' and i['profile']['test'] and i.get('executable')]
    assert len(tests)==1;test=Path(tests[0]['executable']);test_hash=sha(test)
    fixtures=json.loads((RUN/'cases.json').read_text(encoding='utf-8'));results=[]
    for index,f in enumerate(fixtures):
        data=Path(f['data']);backup=Path(f['backup']);case=data.parent
        if index<3:
            retained=json.loads((case/'result.json').read_text(encoding='utf-8'))
            saved=json.loads((case/'before.json').read_text(encoding='utf-8'))
            assert retained['phase']==f['phase'] and retained['currentReaderOpened'] and not (data/'upgrade-hold.json').exists()
            assert snap(backup)==saved['backup'] and all(sha(data/n)==h for n,h in saved['data'].items() if n!='upgrade-hold.json')
            results.append(retained);continue
        resuming=index==3
        if resuming:
            saved=json.loads((case/'before.json').read_text(encoding='utf-8'))
            before,backup_before,payload_before=saved['data'],saved['backup'],saved['payload']
            assert snap(data)==before and snap(backup)==backup_before
        else:
            before=snap(data);backup_before=snap(backup);payload_before=snap(backup/'payload')
            write(case/'before.json',dict(data=before,backup=backup_before,payload=payload_before))
        args=['--from',backup,'--manifest-digest',f['manifestDigest'],'--hold-digest',f['holdDigest'],'--backup-hold-digest',f['backupHoldDigest']]
        reviewed=json.loads((case/'reviewed.json').read_text(encoding='utf-8')) if resuming else cli(data,['recovery','review-previous',*args]);tx=Path(reviewed['transaction'])
        assert reviewed['schema']==2 and reviewed['previousHold']['digest']==f['backupHoldDigest'] and f['holdDigest']!=f['backupHoldDigest']
        if not resuming:write(case/'reviewed.json',reviewed)
        phase=f['phase'];archive=None;child_pid=None
        if phase!='share-denied':
            config=dict(f,reviewDigest=reviewed['digest'],checkpoint=str(case/'checkpoint.json'),token=uuid.uuid4().hex)
            cfg=case/'fault-config.json';write(cfg,config)
            env=os.environ.copy();env['CERULEAN_PREPARATION_FAULT_CONFIG']=str(cfg)
            p=subprocess.Popen([str(test),'--exact','backup_recovery::tests::preparation_interrupt_child','--ignored','--nocapture'],
                               env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=subprocess.CREATE_NO_WINDOW)
            child_pid=p.pid;write(case/'launch.json',dict(pid=p.pid,binary=str(test),sha256=test_hash,configSha256=sha(cfg),testOnlyCheckpoint=True))
            deadline=time.monotonic()+25;checkpoint=None
            while time.monotonic()<deadline:
                if Path(config['checkpoint']).is_file():
                    try:checkpoint=json.loads(Path(config['checkpoint']).read_text(encoding='utf-8'))
                    except json.JSONDecodeError:pass
                if checkpoint or p.poll() is not None:break
                time.sleep(.02)
            if checkpoint!={'phase':phase,'pid':p.pid,'token':config['token'],'testOnlyCheckpoint':True} or p.poll() is not None:
                write(case/'unresolved.json',dict(pid=p.pid,checkpoint=checkpoint,exit=p.poll()));raise RuntimeError('Original checkpoint unproven, no replay')
            cli(data,['upgrade','release','--hold-digest',f['holdDigest']],success=False)
            assert p.poll() is None and (data/'upgrade-hold.json').is_file()
            p.kill()
            try:out,err=p.communicate(timeout=15)
            except subprocess.TimeoutExpired:
                write(case/'termination-unresolved.json',dict(pid=p.pid,automaticRetry=False));raise
            assert p.returncode!=0
            (case/'child.stdout').write_bytes(out);(case/'child.stderr').write_bytes(err)
            write(case/'child-exit.json',dict(pid=p.pid,exit=p.returncode,stdoutEOF=True,stderrEOF=True))
            diagnosis=cli(data,['recovery','diagnose']);assert snap(data)==before
            if phase=='metadata-file-created':
                assert not diagnosis['guardPresent'] and len(diagnosis['unpublishedMetadata'])==1 and not tx.exists()
                orphan=Path(diagnosis['unpublishedMetadata'][0]);assert (orphan/'plan.json').stat().st_size==0
                assert cli(data,['recovery','review-previous',*args])['digest']==reviewed['digest']
                disposition='Unpublished empty metadata preserved; original current hold retained, no preparation replay.'
            else:
                assert diagnosis['guardPresent'] and diagnosis['canAbandonPreparation'] and diagnosis['preparedRecord']=='missing'
                original_tx=snap(tx)
                cli(data,['recovery','apply','--review-digest',reviewed['digest']],success=False)
                assert snap(tx)==original_tx and snap(data)==before
                archive=Path(cli(data,['recovery','abandon','--review-digest',reviewed['digest']]))
                archived=snap(archive)
                assert all(archived.get(n)==h for n,h in original_tx.items()) and set(archived)-set(original_tx)=={'finalization.json'}
                assert snap(data)==before and sha(data/'upgrade-hold.json')==f['holdDigest']
                observed=cli(data,['recovery','inspect','--review-digest',reviewed['digest']])
                assert observed['archives'][0]['verified'] and observed['archives'][0]['disposition']=='abandoned'
                disposition='Original schema2 preparation explicitly abandoned, current hold/newer data retained and evidence archived.'
            final_hold=f['holdDigest'];expected_reader=3
        else:
            lock=None
            if not resuming:
                cli(data,['recovery','prepare-previous',*args,'--review-digest',reviewed['digest']])
                fault=data/'events/00000000.reader3.json'
                lock=kernel.CreateFileW(str(fault),0x80000000,3,None,3,0x80,None)
                assert lock!=ctypes.c_void_p(-1).value,ctypes.get_last_error()
            try:
                refusal=calls[27] if resuming else cli(data,['recovery','apply','--review-digest',reviewed['digest']],success=False)
                assert any(f'(os error {e})' in refusal['stderr'] for e in (5,32)) and snap(data)==before
                state=cli(data,['recovery','status'])
                assert state['originalState']=='original' and state['moveIntent'] and state['stagedBackup'] and not state['preservedOriginal']
                assert state['originalHoldDigest']==f['holdDigest'] and state['restoredHoldDigest']==f['backupHoldDigest']
                before_checks=snap(tx);move=sha(tx/'move-intent.json')
                for action in ('abandon','finish'):cli(data,['recovery',action,'--review-digest',reviewed['digest']],success=False)
                cli(data,['upgrade','release','--hold-digest',f['holdDigest']],success=False)
                assert snap(tx)==before_checks and snap(data)==before
                write(case/'share-failed-observation.json',dict(status=state,error=refusal['stderr'],transaction=before_checks,moveIntentSha256=move,readonly=True))
            finally:
                if lock is not None:assert kernel.CloseHandle(lock)
            write(case/'explicit-continuation.json',dict(reason='Original apply returned failure; no move observed, own deny-delete handle closed. Continue same reviewed transaction once.',originalReview=reviewed['digest'],newPreparation=False))
            restored=cli(data,['recovery','apply','--review-digest',reviewed['digest']])
            assert restored['restorationComplete'] and snap(data)==payload_before and snap(tx/'preserved')==before
            assert sha(tx/'move-intent.json')==move
            archive=Path(cli(data,['recovery','finish','--review-digest',reviewed['digest']]))
            assert snap(archive/'preserved')==before and sha(data/'upgrade-hold.json')==f['backupHoldDigest']
            observed=cli(data,['recovery','inspect','--review-digest',reviewed['digest']]);assert observed['archives'][0]['verified']
            disposition='Actual Windows rename denial under owned deny-delete handle left original directory unchanged; same move-intent continued once after handle closure, old backup restored and newer data preserved.'
            final_hold=f['backupHoldDigest'];expected_reader=f['schema']
        assert snap(backup)==backup_before
        status=cli(data,['data-format','status']);assert status['minimumReaderGeneration']==expected_reader and status['dataSchema']==f['schema']
        cli(data,['upgrade','release','--hold-digest',final_hold])
        opened=command(NEW,['--stdio','--data-dir',data],payload=b'{"id":"previous126","protocolVersion":1,"command":{"type":"snapshot"}}\n')
        assert opened['error'] is None and opened['result']['product']=='CeruleanLonging'
        result=dict(schema=f['schema'],phase=phase,checkpointPid=child_pid,disposition=disposition,archive=str(archive) if archive else None,
                    reviewDigest=reviewed['digest'],originalHold=f['holdDigest'],backupHold=f['backupHoldDigest'],releasedHold=final_hold,
                    expectedReader=expected_reader,backupUnchanged=True,currentReaderOpened=True,automaticReplay=False)
        write(case/'result.json',result);results.append(result)
    write(RUN/'result.json',dict(at=datetime.now(timezone.utc).isoformat(),status='passed',cases=results,calls=len(calls),testBinarySha256=test_hash,
        runtimeSha256=sha(NEW),realModelCalls=0,userApplicationActions=0,productionLogicChanged=False,
        limits=['Initial held/approved data and prior release proof are owned fixtures.',
                'Six preparation interruptions use pre-existing test-only checkpoints extended to prepare_previous; production is uninstrumented.',
                'Two actual production sharing failures resume the same positively observed unchanged transaction; no automatic retry after uncertainty.',
                'No whole-OS/power-loss/disk-full claim; existing apply/finish receipt rules otherwise inherit scoped107–125 evidence.']))
    print(json.dumps(dict(status='passed',cases=len(results),calls=len(calls))))

if __name__=='__main__':main()
