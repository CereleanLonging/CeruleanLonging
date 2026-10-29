"""Real legacy reader, explicit container migration, and original-location rollback.

Only synthetic data in a unique reports directory; no model requests or UI actions.
"""
from pathlib import Path
from hashlib import sha256
import json
import os
import subprocess
import tempfile

root=Path(__file__).resolve().parent.parent
run=Path(tempfile.mkdtemp(prefix='data-format-native-',dir=root/'reports'))
data=run/'data'; project=run/'project'; project.mkdir()
archive=run/'backup'; preserved=run/'preserved-migrated-data'
old=root/'reports/p6-windows-package-014-20261012/cerulean-runtime.exe'
new=root/'target/debug/cerulean-core.exe'
calls=[]; core_started=False; pending=False; results={}
def digest(path):return sha256(path.read_bytes()).hexdigest()
def write(path,value):path.write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n',encoding='utf8')
def command(exe,args,input_value=None,success=True):
    global pending
    payload=None if input_value is None else (json.dumps(input_value,ensure_ascii=False)+'\n').encode('utf8')
    p=subprocess.Popen([str(exe),*map(str,args)],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=subprocess.CREATE_NO_WINDOW,cwd=run)
    try:out,err=p.communicate(payload,timeout=30)
    except subprocess.TimeoutExpired:
        pending=True;write(run/'pending-process.json',{'pid':p.pid,'executable':str(exe),'args':list(map(str,args)),'state':'observation-timeout-do-not-restart'})
        raise RuntimeError('Own process still pending; inspect recorded PID without replay')
    n=len(calls)+1; stem=f'call-{n:03d}'
    (run/(stem+'.stdout')).write_bytes(out);(run/(stem+'.stderr')).write_bytes(err)
    calls.append({'executable':str(exe),'args':list(map(str,args)),'exit':p.returncode,'expectedSuccess':success,'evidence':stem})
    assert (p.returncode==0)==success,(args,p.returncode,err.decode('utf8',errors='replace'))
    return json.loads(out) if success else {'exit':p.returncode,'stderr':err.decode('utf8',errors='replace')}
def cli(exe,*args,success=True):return command(exe,['--workbench','--data-dir',data,*args],success=success)
def stdio(exe,operation,success=True):return command(exe,['--stdio','--data-dir',data],{'id':'migration-fixture','protocolVersion':1,'command':operation},success)
def fingerprint(folder):return {p.relative_to(folder).as_posix():digest(p) for p in folder.rglob('*') if p.is_file()}
try:
    manifest=json.loads((old.parent/'manifest.json').read_text(encoding='utf-8-sig'))
    assert digest(old)==next(x['sha256'] for x in manifest['artifacts'] if x['path']==old.name)
    assert command(new,['--build-info'])['version']=='0.1.5'
    created=stdio(old,{'type':'createPlan','title':'数据迁移验收夹具','tasks':['保留原批准与事件摘要']})
    assert not created.get('error');plan=created['result']['plans'][0]
    approved=stdio(old,{'type':'approvePlan','id':plan['id'],'digest':plan['digest']})
    assert approved['result']['plans'][0]['approved']
    original_plans=approved['result']['plans']
    core_started=True;cli(old,'service','start');cli(old,'project','add','--id','migration-fixture','--root',project,'--timezone','Asia/Shanghai')
    original_workflow=cli(old,'project','show','--project','migration-fixture')['result']['workflow']
    review=cli(old,'upgrade','review')['upgradeReview'];assert review['ready']
    hold=cli(old,'upgrade','prepare','--review-digest',review['digest'])['upgradeHold']['holdDigest'];core_started=False
    saved=cli(new,'upgrade','backup','--hold-digest',hold,'--to',archive)['result'];backup_digest=saved['manifestDigest']
    before=fingerprint(data);assert before==fingerprint(archive/'payload')
    migration_args=['--from',archive,'--manifest-digest',backup_digest,'--hold-digest',hold]
    reviewed=cli(new,'data-format','review',*migration_args)['result'];assert reviewed['fromSchema']==1 and reviewed['toSchema']==2
    cli(new,'data-format','migrate',*migration_args,'--review-digest','0'*64,success=False)
    assert fingerprint(data)==before
    migrated=cli(new,'data-format','migrate',*migration_args,'--review-digest',reviewed['digest'])['result']
    assert migrated['dataSchema']==2 and migrated['sourceBackupDigest']==backup_digest
    after=fingerprint(data);assert set(after)-set(before)=={'events/00000000.json'}
    assert all(after[name]==value for name,value in before.items())
    cli(new,'data-format','migrate',*migration_args,'--review-digest',reviewed['digest'],success=False)
    assert fingerprint(data)==after
    cli(new,'upgrade','release','--hold-digest',hold)
    before_reject=fingerprint(data)
    old_rejection=stdio(old,{'type':'snapshot'},success=False)
    assert fingerprint(data)==before_reject
    snapshot=stdio(new,{'type':'snapshot'});assert snapshot['result']['plans']==original_plans
    core_started=True;cli(new,'service','start')
    assert cli(new,'project','show','--project','migration-fixture')['result']['workflow']==original_workflow
    cli(new,'service','stop');core_started=False
    marker=data/'events/00000000.json';original_marker=marker.read_bytes()
    future=json.loads(original_marker);future['dataSchema']=999;write(marker,future)
    before_reject=fingerprint(data);stdio(new,{'type':'snapshot'},success=False);assert fingerprint(data)==before_reject
    marker.write_bytes(original_marker)
    # Preserve the complete migrated directory; restore only to the original,
    # now absent location. Both resolved targets are inside this owned run.
    assert data.resolve().parent==run.resolve() and preserved.resolve().parent==run.resolve() and not preserved.exists()
    os.rename(data,preserved)
    restored=cli(new,'backup','restore','--from',archive,'--manifest-digest',backup_digest)['result']
    assert restored['manifestDigest']==backup_digest and fingerprint(data)==before
    assert cli(new,'data-format','status')['result']['dataSchema']==1
    # Controlled incomplete-publication fixture, not a real process-crash claim.
    partial=data/'events/.format-migration-fixture.pending';partial.write_bytes(b'{')
    cli(new,'data-format','status',success=False)
    cli(new,'upgrade','release','--hold-digest',hold,success=False)
    assert (data/'upgrade-hold.json').exists()
    partial.unlink()
    cli(old,'upgrade','release','--hold-digest',hold)
    restored_snapshot=stdio(old,{'type':'snapshot'})
    assert restored_snapshot['result']['plans']==original_plans
    result={'status':'passed','run':str(run),'oldVersion':'0.1.4','newVersion':'0.1.5','explicitMigration':True,'wrongReviewNoMutation':True,'originalFileHashesPreserved':True,'approvedPlansPreserved':True,'projectWorkflowPreserved':True,'legacyReaderRejected':True,'legacyRejection':old_rejection,'futureSchemaRejectedBeforeMutation':True,'repeatMigrationRejected':True,'originalLocationRestoreExact':True,'legacyReaderWorksAfterRestore':True,'controlledPartialPublicationBlocksRelease':True,'realCrashTested':False,'holdReleaseExplicit':True,'realModelCalls':0,'userDesktopActions':0,'backupDigest':backup_digest,'migrationReviewDigest':reviewed['digest'],'markerDigest':migrated['markerDigest'],'calls':calls}
except Exception as exc:
    result={'status':'failed','run':str(run),'error':str(exc),'pending':pending,'calls':calls}
finally:
    cleanup='passed'
    if core_started and not pending:
        try:cli(new,'service','stop')
        except Exception as exc:cleanup=str(exc)
    if pending:cleanup='pending-process-preserved'
    result['cleanup']=cleanup;write(run/'result.json',result)
    print(json.dumps(result,ensure_ascii=False))
if result['status']!='passed' or result['cleanup']!='passed':raise SystemExit(1)
