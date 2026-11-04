import { HelpHeading } from "./HelpHeading";
export function WorkerAccountAuthorization({available,checked,disabled,directories=[],onChange}:{available:boolean;checked:boolean;disabled:boolean;directories?:string[];onChange:(checked:boolean)=>void}) {
  return <section className="panel" aria-label="本方案管理员操作授权">
    <HelpHeading as="h3" topic="WorkerAccountAuthorization">隔离账户与目录权限</HelpHeading>
    <label className="checkbox-label"><input type="checkbox" checked={checked} disabled={disabled||!available} onChange={e=>onChange(e.target.checked)}/>授权工作台为本方案创建隔离账户、设置项目目录权限</label>
    <p className="secondary">默认不授权。勾选并确认方案后，工作台自动发起一次准备，仅覆盖当前配置中已登记的模型目录。执行模型仍使用普通账户；模型登录和任务启动另行操作。</p>
    {!!directories.length&&<details><summary>本次保护的主指挥目录及执行目录（{directories.length}）</summary><ul>{directories.map(path=><li key={path}><code>{path}</code></li>)}</ul><p className="secondary">同时保护项目共享目录和工作台内部数据；已登记的闲置模型目录仍保留各自隔离账户。</p></details>}
    <p className="secondary">工作台已有管理员权限时直接使用；否则 Windows 会请求一次 UAC 确认，需你本人操作。取消、失败或重开应用不会自动重弹。目录或方案变化后需要重新审阅。</p>
    <p className="secondary">首次使用此授权会启用数据版本保护，之后须使用 0.1.41 或更新的兼容版本读取该数据目录。</p>
    {!available&&<p className="secondary">当前没有可授权的隔离配置，请先保存主指挥目录及执行目录配置并重新规划。</p>}
  </section>;
}
