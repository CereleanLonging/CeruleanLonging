import { chromium } from "playwright-core";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import { mkdir, mkdtemp, writeFile, readFile, readdir } from "node:fs/promises";
import { resolve, join } from "node:path";
import assert from "node:assert/strict";
const root = resolve("reports");
await mkdir(root, { recursive: true });
const dir = await mkdtemp(join(root, "desktop-smoke-"));
const workflowMode = process.argv.includes("--workflow");
const codexMode = process.argv.includes("--codex");
const openaiVaultMode = process.argv.includes("--openai-vault");
const planningMode = process.argv.includes("--planning");
if(planningMode)assert.ok(workflowMode,"planning test requires workflow registration fixture");
let planningProjectId;
const syntheticKey="sk-native-vault-fixture-never-real";
const codexExecutable = process.env.CERULEAN_TEST_CODEX_EXE;
if(codexMode)assert.ok(codexExecutable,"explicit Codex executable required");
if (workflowMode) {
  const seeded = spawnSync("cargo",["test","-p","cerulean-core","--test","strict_workflow","--locked","--","--ignored","--exact","seed_native_review_fixture"],{
    windowsHide:true,encoding:"utf8",timeout:60000,env:{...process.env,CERULEAN_NATIVE_FIXTURE_DIR:dir},
  });
  assert.equal(seeded.status,0,seeded.stderr);
}
const errors = [];
let app, browser, page;
async function launch() {
  const server = createServer();
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const port = server.address().port;
  await new Promise((r) => server.close(r));
  app = spawn(resolve("target/debug/cerulean-desktop.exe"), [], {
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      CERULEAN_TEST_DATA_DIR: join(dir, "data"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
    },
  });
  app.stderr.on("data", (b) => {
    if (b.toString().includes("panicked")) errors.push(b.toString());
  });
  for (let i = 0; i < 80; i++) {
    if (app.exitCode !== null) throw Error(`Desktop exited ${app.exitCode}`);
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {
        timeout: 1000,
      });
      break;
    } catch {
      await new Promise((r) => setTimeout(r, 250));
    }
  }
  assert.ok(browser, "WebView2 CDP available");
  for (let i = 0; i < 50; i++) {
    page = browser
      .contexts()
      .flatMap((c) => c.pages())
      .find((p) => p.url().includes("tauri.localhost"));
    if (page) break;
    await new Promise((r) => setTimeout(r, 200));
  }
  assert.ok(page, "native Tauri page");
  page.on("pageerror", (e) => errors.push(e.message));
  await page
    .getByText("本地核心已连接", { exact: true })
    .waitFor({ timeout: 20000 });
}
async function close() {
  const exiting = once(app, "exit");
  await page
    .evaluate(() =>
      window.__TAURI_INTERNALS__.invoke("plugin:window|close", {
        label: "main",
      }),
    )
    .catch(() => {});
  await Promise.race([
    exiting,
    new Promise((_, rej) =>
      setTimeout(
        () => rej(Error("Window did not exit normally")),
        8000,
      ).unref(),
    ),
  ]);
  await browser?.close();
  browser = null;
  app = null;
}
try {
  await launch();
  await page.screenshot({ path: join(dir, "overview.png"), fullPage: true });
  assert.equal(await page.title(), "CeruleanLonging");
  await page.getByRole("button", { name: "新建计划" }).click();
  await page.getByLabel("计划名称").fill("基础搭建验收");
  await page
    .getByLabel("具体任务")
    .fill("核对真实桌面与核心通信\n确认批准持久化与重开恢复");
  await page.getByRole("button", { name: "保存计划", exact: true }).click();
  await page.getByRole("button", { name: "审阅并批准", exact: true }).click();
  await page.getByRole("dialog").waitFor();
  await page.getByRole("button", { name: "确认批准", exact: true }).click();
  await page.getByText("批准已写入本地账本。", { exact: true }).waitFor();
  assert.equal(await page.getByText("已批准", { exact: true }).count(), 1);
  await page.screenshot({ path: join(dir, "approved.png"), fullPage: true });
  if (workflowMode) {
    await page.getByRole("button", {name:"审阅执行方案",exact:true}).click();
    const review=page.getByRole("dialog");
    await review.getByRole("heading",{name:"审阅项目执行方案"}).waitFor();
    await review.getByText("$0.25",{exact:true}).waitFor();
    await review.getByText("fixture-validator",{exact:true}).waitFor();
    await review.getByText("副本排除项",{exact:true}).waitFor();
    await review.getByText(".git、target",{exact:true}).waitFor();
    await review.getByText("实际执行程序",{exact:true}).waitFor();
    await review.getByText("程序 SHA-256",{exact:true}).waitFor();
    await review.getByText("openai / fixture-review-model",{exact:true}).waitFor();
    await review.getByText("执行程序 SHA-256",{exact:true}).waitFor();
    const toolCodes=await review.locator("code").allTextContents();
    assert.ok(toolCodes.some(value=>value.endsWith(".exe") && value.includes("strict_workflow")));
    assert.ok(toolCodes.some(value=>/^[a-f0-9]{64}$/.test(value)));
    await review.getByText("使用核心隔离环境，不继承凭证或 PATH",{exact:true}).waitFor();
    await page.screenshot({path:join(dir,"workflow-review.png"),fullPage:true});
    await review.getByText("实际执行程序",{exact:true}).scrollIntoViewIfNeeded();
    await page.screenshot({path:join(dir,"workflow-tool-review.png"),fullPage:true});
    await review.getByRole("button",{name:"确认执行授权"}).click();
    await page.getByText("执行方案已批准",{exact:true}).waitFor();
    const fixture=JSON.parse(await readFile(join(dir,"workflow-fixture.json"),"utf8"));
    const event=JSON.parse(await readFile(join(dir,"data/workflows/project/events/000000000002_user-approve-2.json"),"utf8"));
    assert.equal(event.body.context.execution.approval.revisionDigest,fixture.revisionDigest);
    assert.equal(event.body.event.principal,"user");
    assert.equal(event.body.event.command.kind,"approve");
    const additional=join(dir,"additional-project");await mkdir(additional);
    await page.getByRole("button",{name:"项目设置",exact:false}).click();
    await page.getByLabel("项目目录",{exact:true}).fill(additional);
    await page.getByRole("button",{name:"关联项目目录",exact:true}).click();
    await page.getByLabel("项目目录",{exact:true}).waitFor();
    await page.waitForFunction(()=>document.querySelector("#project-root").value==="");
    const catalogScript=join(dir,"catalog-script.txt");
    await writeFile(catalogScript,"read-only registration fixture\n");
    await page.getByLabel("验收工具名称",{exact:true}).fill("原生工具登记验证");
    await page.getByLabel("程序完整路径",{exact:true}).fill(resolve("target/debug/cerulean-core.exe"));
    await page.getByLabel("固定验收文件（可选，每行一个完整路径）",{exact:true}).fill(catalogScript);
    await page.getByLabel("环境设置（可选）",{exact:true}).fill("CI=true\nNO_COLOR=1");
    await page.getByRole("button",{name:"读取文件并登记",exact:true}).click();
    await page.getByRole("heading",{name:"原生工具登记验证",exact:true}).waitFor();
    const registrations=await readdir(join(dir,"data/validators"));
    assert.equal(registrations.filter(name=>name.endsWith(".json")).length,1);
    const registered=JSON.parse(await readFile(join(dir,"data/validators",registrations.find(name=>name.endsWith(".json"))),"utf8"));
    assert.equal(registered.registration.registeredBy,"user");
    assert.equal(registered.registration.tool.environment.CI,"true");
    assert.equal(registered.registration.tool.inputs.length,1);
    await page.getByRole("heading",{name:"原生工具登记验证",exact:true}).scrollIntoViewIfNeeded();
    await page.screenshot({path:join(dir,"validator-catalog.png"),fullPage:true});
    if(planningMode){
      const before=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke("core_request",{command:{type:"snapshot"}}));
      planningProjectId=before.workflows.find(w=>w.project.root.endsWith("additional-project")).project.id;
      await page.getByRole("button",{name:"需求与计划",exact:false}).click();
      await page.getByLabel("规划项目",{exact:true}).selectOption(planningProjectId);
      await page.getByLabel("项目需求",{exact:true}).fill("实现可独立验收的求和函数");
      await page.getByLabel("主指挥模型 ID",{exact:true}).fill("fixture-chief");
      await page.getByLabel("副指挥模型 ID",{exact:true}).fill("fixture-deputy");
      await page.getByLabel("编程 Codex 程序",{exact:true}).fill(join(dir,"codex.exe"));
      await page.getByLabel("编程模型 ID",{exact:true}).fill("fixture-worker");
      await page.getByLabel("独立验收工具",{exact:true}).selectOption(registered.registration.id);
      await page.getByLabel("验收参数（每行一个参数）",{exact:true}).fill("--fixture\nargument with spaces");
      await page.getByLabel("必须交付的文件（项目相对路径，每行一个）",{exact:true}).fill("sum.mjs");
      await page.getByRole("button",{name:"保存需求与执行配置",exact:true}).click();
      await page.getByText("配置版本 1",{exact:true}).waitFor();
      const after=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke("core_request",{command:{type:"snapshot"}}));
      assert.equal(after.projectSetups.length,1);assert.equal(after.projectSetups[0].setup.worker.model,"fixture-worker");
      assert.deepEqual(after.projectSetups[0].setup.check.args,["--fixture","argument with spaces"]);
      assert.equal(after.workflows.find(w=>w.project.id===planningProjectId).revision,null);
      assert.equal(after.managementDecisions.length,0);
      assert.equal(await page.getByRole("button",{name:"请求主指挥生成方案",exact:true}).isDisabled(),true);
      await page.getByRole("heading",{name:"向主指挥提交需求",exact:true}).scrollIntoViewIfNeeded();
      await page.screenshot({path:join(dir,"project-planning.png"),fullPage:true});
    }
  }
  await page.getByRole("button", { name: "接入管理", exact: false }).click();
  if(openaiVaultMode){
    const field=page.getByLabel("OpenAI API Key",{exact:true});
    assert.equal(await field.getAttribute("type"),"password");
    await field.fill(syntheticKey);
    await page.getByRole("button",{name:"加密保存 Key",exact:true}).click();
    await page.getByText("已加密保存，待检查",{exact:true}).waitFor();
    assert.equal(await field.inputValue(),"");
    const encrypted=await readFile(join(dir,"data/credentials/openai.dpapi"));
    assert.equal(encrypted.includes(Buffer.from(syntheticKey)),false);
    const snapshot=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke("core_request",{command:{type:"snapshot"}}));
    assert.equal(snapshot.openaiConnection.keyConfigured,true);
    assert.equal(snapshot.openaiConnection.state,"unchecked");
    assert.ok(!JSON.stringify(snapshot).includes(syntheticKey));
    await page.getByRole("heading",{name:"OpenAI 主副指挥",exact:true}).scrollIntoViewIfNeeded();
    await page.screenshot({path:join(dir,"openai-key-saved.png"),fullPage:true});
  }
  if(codexMode) {
    await page.getByLabel("Codex 程序完整路径",{exact:true}).fill(codexExecutable);
    await page.getByRole("button",{name:"检查现有 Codex 登录",exact:true}).click();
    await page.getByText("连接检查通过",{exact:true}).waitFor({timeout:60000});
    await page.getByText("登录类型：chatgpt",{exact:true}).waitFor();
    const snapshot=await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke("core_request",{command:{type:"snapshot"}}));
    assert.equal(snapshot.codexConnection.state,"available");
    assert.ok(snapshot.codexConnection.models.length>0);
    assert.equal(snapshot.verificationJobs.length,0);
    await writeFile(join(dir,"codex-connection.json"),JSON.stringify({status:"passed",check:snapshot.codexConnection,modelCalls:0},null,2));
    await page.screenshot({path:join(dir,"codex-connection.png"),fullPage:true});
  }
  assert.equal(await page.locator(".adapter-grid article").count(), 10);
  assert.equal(
    await page
      .locator(".adapter-grid .badge")
      .filter({ hasText: "待接入" })
      .count(),
    10,
  );
  await close();
  const surviving = spawnSync(resolve("target/debug/cerulean-core.exe"),["--request","--data-dir",join(dir,"data")],{
    windowsHide:true,encoding:"utf8",timeout:20000,input:JSON.stringify({id:"after-window-close",protocolVersion:1,command:{type:"snapshot"}}),
  });
  assert.equal(surviving.status,0,surviving.stderr);
  assert.equal(JSON.parse(surviving.stdout).response.result.plans[0].approved,true);
  await launch();
  if(openaiVaultMode){
    await page.getByRole("button",{name:"接入管理",exact:false}).click();
    await page.getByText("已加密保存，待检查",{exact:true}).waitFor();
    assert.equal(await page.getByLabel("OpenAI API Key",{exact:true}).inputValue(),"");
    await page.getByRole("button",{name:"删除本机 Key",exact:true}).click();
    await page.getByText("尚未配置",{exact:true}).waitFor();
    assert.equal((await readdir(join(dir,"data/credentials"))).includes("openai.dpapi"),false);
  }
  await page.getByRole("button", { name: "需求与计划", exact: false }).click();
  await page.getByText("基础搭建验收", { exact: true }).waitFor();
  assert.equal(await page.getByText("已批准", { exact: true }).count(), 1);
  if(workflowMode)assert.equal(await page.getByText("执行方案已批准",{exact:true}).count(),1);
  if(planningMode){
    await page.getByLabel("规划项目",{exact:true}).selectOption(planningProjectId);
    assert.equal(await page.getByLabel("项目需求",{exact:true}).inputValue(),"实现可独立验收的求和函数");
    assert.equal(await page.getByLabel("编程模型 ID",{exact:true}).inputValue(),"fixture-worker");
    await page.getByText("配置版本 1",{exact:true}).waitFor();
  }
  if(workflowMode) {
    await page.getByRole("button",{name:"项目设置",exact:false}).click();
    await page.getByRole("heading",{name:"原生工具登记验证",exact:true}).waitFor();
  }
  await page.getByRole("button", { name: "交接记录", exact: false }).click();
  assert.equal(await page.locator(".event").count(), 2);
  await page.screenshot({ path: join(dir, "reopened.png"), fullPage: true });
  const events = await readdir(join(dir, "data/events"));
  assert.equal(events.filter((x) => x.endsWith(".json")).length, 2);
  const approved = JSON.parse(
    await readFile(join(dir, "data/events/00000002.json"), "utf8"),
  );
  assert.equal(approved.body.change.kind, "planApproved");
  assert.deepEqual(errors, []);
  await close();
  const result = {
    at: new Date().toISOString(),
    status: "passed",
    root: dir,
    checks: {
      nativeTauriWindow: true,
      realRustCore: true,
      createPlan: true,
      explicitApproval: true,
      restartPersistence: true,
      residentSurvivesWindowClose: true,
      adaptersTruthfullyUnconnected: true,
      twoPersistedEvents: true,
      noPageErrors: true,
    },
    screenshots: ["overview.png", "approved.png", "reopened.png"],
    planningChecks:planningMode?{nativeConfiguration:true,pinnedWorkerAndValidator:true,argumentsPreserved:true,saveDoesNotApproveOrGenerate:true,reopenPreserved:true,missingKeyDisablesRequest:true,modelCalls:0}:null,
    workflowChecks: workflowMode ? {syntheticChiefProposal:true,fullRevisionDialog:true,approvedWorkerModelAndExecutable:true,approvedValidatorPathHashEnvironment:true,digestBoundApproval:true,projectRegistration:true,validatorRegistrationPersisted:true,validatorRegistrationAfterReopen:true,approvalAfterReopen:true,modelCalls:0} : null,
    codexChecks: codexMode ? {realAppServer:true,existingLogin:true,modelDiscovery:true,asyncRefresh:true,noModelDispatch:true} : null,
    openaiVaultChecks:openaiVaultMode ? {maskedInput:true,inputCleared:true,encryptedOnDisk:true,noPlaintextSnapshot:true,preservedAcrossWindowReopen:true,explicitRemoval:true,apiCalls:0,syntheticCredentialOnly:true}:null,
  };
  await writeFile(join(dir, "result.json"), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
} catch (e) {
  if (page)
    await page
      .screenshot({ path: join(dir, "failure.png"), fullPage: true })
      .catch(() => {});
  await writeFile(
    join(dir, "failure.json"),
    JSON.stringify({ error: String(e), errors }, null, 2),
  );
  throw e;
} finally {
  await browser?.close().catch(() => {});
  app?.kill();
  const stopped=spawnSync(resolve("target/debug/cerulean-core.exe"),["--stop","--data-dir",join(dir,"data")],{windowsHide:true,encoding:"utf8",timeout:20000});
  if(stopped.status!==0)console.error("Isolated test service cleanup:",stopped.stderr);
}
