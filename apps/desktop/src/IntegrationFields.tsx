import type { IntegrationDraft, RegisteredValidator } from "./contracts";

export function IntegrationFields({value, validators, onChange}: {
  value: IntegrationDraft | undefined;
  validators: RegisteredValidator[];
  onChange: (value: IntegrationDraft | undefined) => void;
}) {
  return <section className="panel" aria-label="项目整体验收">
    <h3>项目整体验收</h3>
    <label className="checkbox-label"><input type="checkbox" checked={!!value} onChange={e=>onChange(e.target.checked?{taskId:"integration",artifactPaths:[],validatorId:validators[0]?.id??"",args:[],timeoutSeconds:30}:undefined)}/>单独安排最终集成任务</label>
    {value&&<>
      <p className="secondary">主指挥方案必须包含以下任务，并覆盖所有其他任务的成果。集成任务使用独立目录和下列整体检查；通过后由主指挥收口。检查脚本应验证合并后的完整功能。保存后仍需重新规划、批准并启动。</p>
      <label htmlFor="integration-task">最终任务 ID</label><input id="integration-task" required value={value.taskId} onChange={e=>onChange({...value,taskId:e.target.value})}/>
      <label htmlFor="integration-artifacts">最终交付文件（相对路径，每行一个）</label><textarea id="integration-artifacts" required value={value.artifactPaths.join("\n")} onChange={e=>onChange({...value,artifactPaths:e.target.value.split(/\r?\n/)})}/>
      <label htmlFor="integration-validator">整体检查工具</label><select id="integration-validator" required value={value.validatorId} onChange={e=>onChange({...value,validatorId:e.target.value})}><option value="">选择已登记工具</option>{validators.map(v=><option key={v.id} value={v.id}>{v.name}</option>)}</select>
      <label htmlFor="integration-args">整体检查参数（每行一个）</label><textarea id="integration-args" value={value.args.join("\n")} onChange={e=>onChange({...value,args:e.target.value?e.target.value.split(/\r?\n/):[]})}/>
      <label htmlFor="integration-seconds">整体检查时限（秒）</label><input id="integration-seconds" required type="number" min={1} max={3600} value={value.timeoutSeconds} onChange={e=>onChange({...value,timeoutSeconds:Number(e.target.value)})}/>
    </>}
  </section>;
}
