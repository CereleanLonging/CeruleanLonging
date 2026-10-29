// Static React approval rendering only; no Tauri, IPC, provider or model request.
import {readFile,writeFile,mkdtemp} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {pathToFileURL} from 'node:url';
import assert from 'node:assert/strict';
import {transformWithOxc} from 'vite';
import {createElement} from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {chromium} from 'playwright-core';
assert.equal(process.env.CERULEAN_NATIVE_OWNED_JOB,'1','run in private owned Job');
const dir=await mkdtemp(resolve('reports/api-approval-render-'));
const source=await readFile('apps/desktop/src/ApiExecutionReview.tsx','utf8');
const module=(await transformWithOxc(source,resolve('apps/desktop/src/ApiExecutionReview.tsx'),{jsx:{runtime:'automatic'}})).code;
const modulePath=join(dir,'component.mjs');await writeFile(modulePath,module);
const {ApiExecutionReview}=await import(pathToFileURL(modulePath).href);
const worker={target:{kind:'provider',endpoint:{kind:'qwenBeijing',workspaceId:'workspace-123'}},model:'approved-model-example',fileTools:['read-files'],maxRounds:4,maxToolCalls:12,maxOutputTokens:2048,maxTotalTokens:20000,requestTimeoutSeconds:45};
const markup=renderToStaticMarkup(createElement(ApiExecutionReview,{worker}));
assert.ok(markup.includes('Qwen 北京')&&markup.includes('workspace-123')&&markup.includes('approved-model-example'));
assert.ok(markup.includes('读取项目文件')&&!markup.includes('写入项目文件'));
assert.ok(markup.includes('4 轮模型请求')&&markup.includes('12 次文件调用')&&markup.includes('45 秒'));
assert.ok(markup.includes('可能使累计值超过阈值')&&markup.includes('不支持金额硬上限'));
const html=`<!doctype html><html lang="zh-CN"><meta charset="utf-8"><title>API 授权内容静态核查</title><style>body{font-family:system-ui,"Microsoft YaHei",sans-serif;background:#f4f6fb;color:#172033;margin:0;padding:40px}main{max-width:880px;background:white;border:1px solid #dae0ed;border-radius:16px;padding:28px;margin:auto}h1{font-size:24px}dl{display:grid;grid-template-columns:150px 1fr;gap:18px 22px;line-height:1.7}dt{font-weight:600}dd{margin:0;overflow-wrap:anywhere}small{color:#58647c}</style><main><small>隔离渲染核查 · 不代表原生产品执行验收</small><h1>API 执行授权</h1>${markup}</main></html>`;
await writeFile(join(dir,'review.html'),html);
let browser;
try {
  browser=await chromium.launch({executablePath:'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
  const page=await browser.newPage({viewport:{width:1100,height:780},deviceScaleFactor:1});
  await page.route('**/*',route=>route.abort());
  await page.setContent(html);await page.screenshot({path:join(dir,'review.png'),fullPage:true});
} finally {await browser?.close();}
const result={status:'passed',scope:'isolated static React rendering, not native product integration',providerRequests:0,modelCalls:0,artifact:dir,readOnlyPermissionShown:true,limitsAndThresholdCaveatShown:true};
await writeFile(join(dir,'result.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
