import { HelpHeading } from "./HelpHeading";
import {IndependentVerifier} from "./IndependentVerifier";
import type {Command,RegisteredValidator} from "./contracts";
export function ValidatorCatalog({connected}:{items:RegisteredValidator[];send:(command:Command)=>Promise<boolean>;busy:boolean;connected:boolean}) {
  return <section className="panel validator-catalog" aria-label="固定检查与验收工具">
    <HelpHeading as="h2" topic="ValidatorCatalog">检查与盲评验收</HelpHeading>
    <IndependentVerifier connected={connected} checker/>
    <IndependentVerifier connected={connected}/>
    <p className="secondary">独立检查执行批准的客观测试，单向交报告给独立盲评验收工具。两个程序职责分离，检查入口唯一，不开放新增或替换。历史外部登记仅在后端保留。</p>
  </section>;
}
