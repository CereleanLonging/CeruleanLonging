"""Read current evidence and append a bounded progress handoff; no product actions."""
from pathlib import Path
from datetime import datetime, timezone, timedelta
import hashlib
import json
import re

ROOT = Path(__file__).resolve().parents[1]
NOW = datetime.now(timezone(timedelta(hours=8)))
OUT = ROOT/'reports'/('progress-handoff-'+NOW.strftime('%Y%m%d-%H%M%S'))
OUT.mkdir(exist_ok=False)
def read(p): return json.loads(Path(p).read_text(encoding='utf-8-sig'))
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(name, data):
    with (OUT/name).open('x',encoding='utf-8') as f:
        json.dump(data,f,ensure_ascii=False,indent=2)

docs=['docs/HANDOFF-LATEST.md','docs/NEXT-STAGE-GATES.md','docs/DELIVERY-ACCEPTANCE-190.md']
before={p:(ROOT/p).read_bytes() for p in docs}
for p, data in before.items():
    target=OUT/'before'/p; target.parent.mkdir(parents=True,exist_ok=True)
    target.write_bytes(data)
baseline=read(ROOT/'reports/delivery-audit-190/source.json')['currentInputs']
source_changes=[p for p,h in baseline.items() if not (ROOT/p).is_file() or sha(ROOT/p)!=h]
assert not source_changes, source_changes

steps=[]; evidence={}
for folder in sorted((ROOT/'reports/delivery-acceptance-190').glob('step*')):
    if not folder.is_dir(): continue
    result=read(folder/'result.json'); summary=read(folder/'test-summary-final.json')
    names=set(); receipts=[]
    for p in sorted(folder.glob('*.receipt.json')):
        r=read(p)
        if not all(k in r for k in ('stdout','stderr','stdoutSha256','stderrSha256')): continue
        for stream in ('stdout','stderr'):
            assert sha(ROOT/r[stream])==r[stream+'Sha256'], str(p)
            evidence[r[stream]]=r[stream+'Sha256']
        log=(ROOT/r['stdout']).read_text(encoding='utf-8-sig')
        actual_names=re.findall(r'^test (\S+) \.\.\. ok\s*$',log,re.MULTILINE)
        if r.get('accepted'):
            assert r['exitCode']==0 and not r.get('error') and r.get('ended')
            assert actual_names and any(n>0 for n in r['running'])
            assert all(x['failed']==0 and x['ignored']==0 for x in r['summaries'])
            names.update(actual_names)
        receipts.append({'path':p.relative_to(ROOT).as_posix(),'sha256':sha(p),'accepted':r.get('accepted',False),'exitCode':r['exitCode']})
        evidence[p.relative_to(ROOT).as_posix()]=sha(p)
    assert len(names)==summary['uniquePassed']
    assert names==set(summary.get('passedTestNames', summary.get('tests', [])))
    assert result['productionSourceChanged'] is False and result['releaseAccepted'] is False
    for name in ('result.json','test-summary-final.json','verification.json'):
        p=folder/name; evidence[p.relative_to(ROOT).as_posix()]=sha(p)
    steps.append({'directory':folder.relative_to(ROOT).as_posix(),'status':result['status'],'verifiedPriorPassedTests':len(names),'acceptedCommands':sum(r['accepted'] for r in receipts),'excludedCommands':sum(not r['accepted'] for r in receipts),'receipts':receipts,'testsRerun':False})
assert len(steps)==2

package=ROOT/'reports/p6-windows-package-0112-20261017'
manifest=read(package/'manifest.json'); ready=read(ROOT/'reports/candidate-0112-189/candidate-ready.json')
assert sha(package/'manifest.json')==ready['manifestSha256']
for a in manifest['artifacts']:
    p=package/a['path']; assert sha(p)==a['sha256'] and p.stat().st_size==a['bytes']
    evidence[p.relative_to(ROOT).as_posix()]=sha(p)
signature=read(ROOT/'reports/delivery-audit-190/signature-audit-v2.json')
observations=list(signature['releaseArtifacts'])
for p in signature['packages']: observations.extend(a['observed'] for a in p['artifacts'])
signature_drift=[a['path'] for a in observations if not Path(a['path']).is_file() or sha(a['path'])!=a['sha256']]
assert not signature_drift, signature_drift
for p in ('reports/delivery-audit-190/signature-audit-v2.json','reports/delivery-audit-190/signature-summary.json','reports/delivery-audit-190/result.json','reports/p6-windows-package-0112-20261017/manifest.json'):
    evidence[p]=sha(ROOT/p)
save('evidence.json',evidence)
save('source-check.json',{'baseline':'reports/delivery-audit-190/source.json','compared':len(baseline),'differences':source_changes,'scope':'Existing indexed production inputs; no new runtime invocation or full filesystem immutability claim'})
save('result.json',{'at':NOW.isoformat(),'status':'progress-evidence-reconciled','steps':steps,'productionInputsCompared':len(baseline),'sourceChanges':source_changes,'candidateVersion':manifest['version'],'candidateManifestSha256':sha(package/'manifest.json'),'candidateIncludesFrontend190Fix':False,'signatureEvidenceAt':signature['at'],'signatureEvidenceObservations':len(observations),'signatureArtifactDrift':signature_drift,'signaturesReaudited':False,'testsRunThisAudit':0,'realProviderCalls':0,'realDesktopActions':0,'releaseAccepted':False})

folder=ROOT/'handoffs'/NOW.strftime('%Y-%m-%d');folder.mkdir(exist_ok=True)
numbers=[int(m.group(1)) for p in folder.glob('handoff_*.md') if (m:=re.search(r'handoff_\d{4}-\d{2}-\d{2}_(\d+)_',p.name))]
number=max(numbers,default=0)+1
handoff=folder/f'handoff_{NOW:%Y-%m-%d}_{number:03d}_{NOW:%H%M%S%z}.md'
rel=handoff.relative_to(ROOT).as_posix(); outrel=OUT.relative_to(ROOT).as_posix()
text=f'''# 最新进度与验收状态核对 / 190 / {NOW.isoformat()}

承接011桌面核验及010 OMP核验。本次按用户要求检查进度与更新交接；未修改产品源码、构建安装包、调用模型或操作桌面，不重跑已通过测试。

## 最新进度

| 范围 | 已取得的证据 | 当前结论 |
| --- | --- | --- |
| OMP步骤1 | 13个既有专项、6条成功命令，失败记录保留；本次重核原日志SHA与实际测试名 | 原工具/任务ACK/恢复基础已核验，主动监督未接通 |
| 桌面步骤2 | 17个既有专项、13条成功命令；零命中和默认ignored两次不算通过 | 任务身份、回复关联、用户停止与恢复基础已核验，主动建议/自动停止未接通 |
| 前端190修复 | 版本与接入展示修正、默认大分块警告消除；原10项UI与构建证据继承 | 开发源码已包含，冻结0.1.12包尚未包含 |
| 内部候选 | 0.1.12 reader12、三产物与原清单SHA/大小一致 | 仍为内部候选；不能代证安装GUI完整流程 |
| 签名 | 核对最近190签名审计涉及的88项产物字节未漂移 | 继承{signature['at']}的未签名结论，本轮未重新执行WinVerifyTrust或查询证书存储 |

本次核对190记录的{len(baseline)}项既有生产输入，摘要均未变化；两组全部23份命令回执及日志摘要吻合，13/17测试计数与原stdout测试名一致。4份失败/零命中/默认ignored回执保留且不计通过；本轮实际测试执行数为0。候选1026通过/216忽略、阶段08选定145和上述专项有覆盖重叠，不相加成总覆盖数。

## 关键判断与待办

1. OMP安装树17.2.14与历史18.4.4不同、ACP源码与产品严格RPC字段不符，是010静态核验发现，**不是本轮真实握手失败**。普通steer success不足以证明建议进入原任务。下一步先固定实际产品入口/版本/协议，复用原管理请求和原任务唯一所有者完成监督接线，再做增量故障验证；不能仅解除拒绝条件。
2. 桌面当前绑定以应用/窗口/任务/历史为依据，WorkBuddy显示模型标签不等于完整模型配置认证。现有Assign/Accept、用户停止及原型20/20均保留，但不代证主动监督。下一步需补建议生成、原任务投送及ACK/未知恢复的产品实现；真实验证再按指定会话与具体授权执行。
3. 010中的“本轮禁止Git/不实现”是该次核验的执行范围，不自动成为后续工程的永久限制。新增实现、隔离夹具与本地差异核验可按既有项目授权推进；真实发送、用户活动数据升级及密钥/信任操作仍须遵守具体授权。不能把尚未实现的产品接线全部归类为等待用户环境。
4. 后续保持原实施范围：首批真实能力与预算、生产金额放行、同负载效率/可靠性、包含前端修复的下一候选、安装版完整流程/整包更新回退、独立干净Windows、正式签名。用户已说明暂缺独立环境/正式证书，不重复询问。当前本地测试证书与正式发布身份分开。

本次仅核对当前索引内源码与指定证据，不宣称完整bug审计、全部文件不可变或新实机验收。两份旧0.1.0清单缺coreBuildInfo仍作为历史元数据缺项保留。正式验收路径见docs/DELIVERY-ACCEPTANCE-190.md，步骤1/2仍未完整签收，releaseAccepted=false。

证据：{outrel}/result.json、evidence.json、source-check.json。原文档完整字节前置保留；本次没有新Key读取、供应商联网、签名、信任改变或安装。
'''
summary=f'''# 最新进度汇总 / 190 / {NOW.isoformat()}

已复核010/011新增成果：OMP13项、桌面17项既有专项通过，原回执与日志SHA/测试名一致；均为基础机制核验，主动监督仍未实现。190记录的{len(baseline)}项生产输入无差异，0.1.12三产物与清单匹配；前端修复仍未入旧包。继承最近签名审计，相关88项产物字节未漂移，本轮不冒称重新审签。未重跑测试、未真实发送。

后续先处理OMP实际协议兼容与监督接线、桌面建议/ACK与恢复，再推进真实矩阵、候选/安装更新及正式发布条件。上一轮只读范围不成为永久禁止实现的规则，缺少产品接线也不能全部归因外部环境。阶段1/2与正式验收均未签收。详见[本次交接](../{rel})、[核对证据](../{outrel}/result.json)及[正式验收路径](DELIVERY-ACCEPTANCE-190.md)。releaseAccepted=false。

---

'''
status=f'''# 状态更新 / {NOW.isoformat()}

步骤1 OMP和步骤2桌面已完成基础机制核验，分别有13/17项既有专项证据；**两项主动监督仍未实现，尚未完整签收**。生产输入相对190修复无变化。0.1.12冻结候选保留，尚未包含前端190修复；安装版完整流程、真实能力/费用/效率、独立Windows与正式签名继续待验。下一步以产品接线与实际协议兼容为主，避免再重复基础接入验证。最新核对见[交接](../{rel})。

---

'''
for p,b in before.items(): assert (ROOT/p).read_bytes()==b, 'Concurrent modification: '+p
assert all(sha(ROOT/p)==h for p,h in baseline.items())
assert all(sha(ROOT/p)==h for p,h in evidence.items())
with handoff.open('x',encoding='utf-8') as f:f.write(text)
for p in docs:
    prefix=status if p.endswith('DELIVERY-ACCEPTANCE-190.md') else summary
    assert (ROOT/p).read_bytes()==before[p]
    (ROOT/p).write_bytes(prefix.encode('utf-8')+before[p])
    assert (ROOT/p).read_bytes().endswith(before[p])
save('publication.json',{'at':NOW.isoformat(),'handoff':rel,'historicalBytesPreserved':True,'files':{p:sha(ROOT/p) for p in docs+[rel]},'releaseAccepted':False})
print(json.dumps({'report':outrel,'handoff':rel,'verifiedPriorTests':[s['verifiedPriorPassedTests'] for s in steps],'currentTestsRun':0,'releaseAccepted':False}))
