import { HelpHeading } from "./HelpHeading";
import type { Command, Snapshot } from "./contracts";
import { ExperimentWorkbenches } from "./ExperimentWorkbenches";
import { TeamTimingPanel } from "./TeamTimingPanel";
import { BaselinePanel } from "./BaselinePanel";
import { ErrorText } from "./ErrorText";

type Props = {
  desktop: boolean; enabled: boolean; onEnabled: (enabled: boolean) => void;
  data: Snapshot | null; busy: boolean; loading: boolean; error: string;
  refresh: () => void; send: (command: Command) => Promise<boolean>;
};

export function ExperimentLab(props: Props) {
  return <section className="experiment-lab" aria-label="独立实验功能">
    <div className="page-heading"><div><div className="eyebrow">EXPERIMENT WORKBENCH</div><HelpHeading as="h1" topic="ExperimentLab">独立实验工作台</HelpHeading><p>集中管理独立实验窗口，以及团队与单执行者的效率对照。</p></div></div>
    <section className="panel">
      <label className="experiment-feature-switch"><input type="checkbox" checked={props.enabled} disabled={props.busy} onChange={event => props.onEnabled(event.target.checked)}/>开启独立实验功能</label>
      <p>每次打开工作台默认关闭，勾选后展开实验管理与效率验收。开启只显示功能，不会创建实验或发送模型任务。</p>
      <p className="secondary">取消勾选只收起功能；已打开的实验窗口与已开始的任务仍需在原窗口中处理。</p>
    </section>
    {props.enabled && <>
      <ExperimentWorkbenches desktop={props.desktop} inline/>
      {!props.desktop ? <p>请在桌面应用中配置和执行实验。</p> : props.loading ? <p role="status">正在读取当前工作台的实验记录…</p> : props.error ? <div role="alert"><ErrorText error={props.error}/><button className="secondary-button" disabled={props.busy} onClick={props.refresh}>重新读取实验记录</button></div> : props.data?.coreDiagnostic ? <p role="alert">当前核心需要诊断，请返回工作总览核对原记录后再使用效率验收。</p> : props.data && <div className="plan-layout">
        <TeamTimingPanel data={props.data} busy={props.busy} send={props.send} desktop={props.desktop}/>
        <BaselinePanel data={props.data} busy={props.busy} send={props.send}/>
      </div>}
    </>}
  </section>;
}
