import type {AcceptanceCheck} from "./contracts";
export function ObjectiveTestReview({check}:{check:AcceptanceCheck}) {
  if(check.args[0]!=="--approved-script-v1")return <code>{[check.program,...check.args].join(" ")}</code>;
  let recipe:{test:{language:string;source:string};runtime:{path:string;sha256:string}};
  try {
    recipe=JSON.parse(check.args.slice(1).join(""));
    if(typeof recipe.test?.source!=="string"||typeof recipe.runtime?.path!=="string")throw Error();
  } catch {return <p role="alert">客观测试方案无法解析，请返回主指挥修订，勿批准执行。</p>;}
  return <section aria-label="待批准的客观测试源码">
    <p>独立检查工具执行以下 {recipe.test.language==="python"?"Python":"JavaScript"} 测试。请确认测试覆盖需求，且没有联网、安装依赖或修改源码的操作。</p>
    <details open><summary>审阅测试源码</summary><pre><code>{recipe.test.source}</code></pre></details>
    <p>批准运行时：<code>{recipe.runtime.path}</code></p>
    <p className="secondary">测试源码、断言、运行时摘要与本版方案一同批准。修改后须重新审阅。</p>
  </section>;
}
