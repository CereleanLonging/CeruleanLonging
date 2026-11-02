export function AssertionFields({id,value,onChange}:{id:string;value?:string[];onChange:(value:string[])=>void}) {
  return <div className="assertion-fields">
    <label htmlFor={id}>必须通过的断言 ID（每行一个）</label>
    <textarea id={id} rows={3} value={(value??[]).join("\n")} onChange={e=>onChange(e.target.value.split(/\r?\n/))} placeholder={"normal-case\nedge-case\ninvalid-input"}/>
    <p className="secondary">启用角色隔离的项目必须填写。检查器须输出 JSON 断言报告；缺项、重复、失败或仅打印“成功”均不通过。ID 应对应实际需求和测试断言。</p>
  </div>;
}

export function cleanAssertions(values?:string[]):string[]|undefined {
  const ids=(values??[]).map(v=>v.trim()).filter(Boolean);
  return ids.length?ids:undefined;
}
