"""Actual new controller reads legacy journal without adopting its transaction."""
import importlib.util
import json
from pathlib import Path
root=Path(__file__).resolve().parent.parent
spec=importlib.util.spec_from_file_location('driver',root/'tests/check-update-controller-123-valid.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
source=m.RUN
m.RUN=root/'reports/update-reader-123-legacy-review'
m.RUN.mkdir()
fixture=json.loads((source/'reviewed-fixture.json').read_text(encoding='utf-8'))
plan=json.loads((Path(fixture['transaction'])/'plan.json').read_text(encoding='utf-8'))
tx=m.RUN/'transaction';tx.mkdir();(tx/'transaction.lock').write_bytes(b'')
plan['schema']=1;plan['transaction']=str(tx)
plan.pop('controllerReaderGeneration')
# Keep legacy interlock=1 and an unavailable helper: read-only Review must not launch it.
plan['maintenanceCore']=str(m.RUN/'must-not-launch.exe')
digest=m.write(tx/'plan.json',plan)
m.write(tx/'event-001.json',dict(schema=1,sequence=1,previousDigest=digest,kind='prepared'))
before=m.snapshot(tx)
def review(action,reject=''):
    return m.call(m.PS,['-NoProfile','-NonInteractive','-ExecutionPolicy','RemoteSigned','-File',root/'scripts/manage-windows-update.ps1','-Action',action,'-TransactionDirectory',tx,'-PlanDigest',digest],reject)
r=review('Review')
assert r['state']=='prepared' and r['maintenance'] is None
for action in ('Rollback','FinishMaintenance'):
    review(action,'Legacy transaction requires its original controller')
assert m.snapshot(tx)==before
m.write(m.RUN/'result.json',dict(status='passed',calls=len(m.calls),transactionBytesUnchanged=True,missingHelperNotLaunched=True,legacyMutationRejected=True,review=r,installed=False,limits='Controlled legacy journal; no actual installed rollback or commit.'))
print('Legacy Review passed; two mutation routes refused without changes.')
