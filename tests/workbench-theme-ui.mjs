// Exercise the production shell against isolated IPC fixtures, without model calls.
import assert from 'node:assert/strict';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { build } from 'vite';
import { chromium } from 'playwright-core';

assert(process.env.CERULEAN_CACHE_SESSION, 'Use the managed cache runner');
const report = await mkdtemp(resolve('reports/workbench-theme-ui-'));
const bundle = join(process.env.CERULEAN_CACHE_SESSION, 'workbench-theme-ui');
const entry = join(report, 'entry.tsx');
await writeFile(entry, `import React from 'react';
import { createRoot } from 'react-dom/client';
import { App } from '../../apps/desktop/src/App';
import '../../apps/desktop/src/style.css';
createRoot(document.getElementById('root')).render(<App />);`);
await build({
  configFile: false, logLevel: 'error',
  define: { 'process.env.NODE_ENV': JSON.stringify('production') },
  oxc: { jsx: { runtime: 'automatic' } },
  build: { outDir: bundle, emptyOutDir: false,
    lib: { entry, name: 'WorkbenchTheme', formats: ['iife'], fileName: () => 'ui.js', cssFileName: 'ui' } },
});

const snapshot = {
  dataDirectory: 'isolated-theme-fixture', events: [],
  workflows: [{ project: { id: 'theme-fixture', root: 'D:/synthetic/theme', timezone: 'UTC' }, state: { tasks: {} }, tasks: [], approval: null }],
  projectSetups: [], workerAccountSetups: [], validators: [], managementDecisions: [],
  managementHistory: { projects: [] }, openaiConnection: { models: [], keyConfigured: false },
  codexConnection: { models: [], executable: '' }, providerConnections: [], projectRuns: [],
  codexJobs: [], apiJobs: [], ompJobs: [], verificationJobs: [], desktopBindings: [], adapters: [],
};
const cases = [];
const errors = [];
let externalRequests = 0;
let browser;
try {
  browser = await chromium.launch({ executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe', headless: true });
  for (const mode of ['daily', 'experiment']) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    page.setDefaultTimeout(10000);
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/*', route => route.request().url() === 'http://localhost/theme-fixture'
      ? route.fulfill({ contentType: 'text/html', body: '<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>' })
      : (externalRequests++, route.abort()));
    await page.goto('http://localhost/theme-fixture');
    await page.evaluate(({ mode, snapshot }) => {
      window.fixtureCalls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: mode === 'experiment' ? 'experiment-exp-theme' : 'main' } },
        transformCallback: () => 1, unregisterCallback() {},
        invoke: async (name, args) => {
          window.fixtureCalls.push(name);
          if (name === 'plugin:event|listen') return 1;
          if (name === 'plugin:event|unlisten' || name === 'cancel_workbench_exit') return;
          if (name === 'core_request') return structuredClone(snapshot);
          if (name === 'independent_verifier_status') return { ready: false, message: 'offline fixture' };
          if (name === 'workspace_read') {
            const type = args.query.type;
            if (type === 'overview') return { type, value: { product: 'fixture', protocolVersion: 1, dataDirectory: snapshot.dataDirectory, mode: 'ready', legacyPlanCount: 0, pendingLegacyPlanCount: 0, projectCount: 1 } };
            if (type === 'projects' || type === 'plans') return { type, value: { revision: 'fixture', offset: 0, total: 0, nextOffset: null, entries: [] } };
          }
          throw Error('Unexpected fixture IPC ' + name);
        },
      };
    }, { mode, snapshot });
    await page.addStyleTag({ path: join(bundle, 'ui.css') });
    await page.addScriptTag({ path: join(bundle, 'ui.js') });
    await page.getByText('本地核心已连接', { exact: true }).waitFor();
    await page.evaluate(() => document.fonts.ready);
    assert.equal(await page.locator('.shell').getAttribute('data-workbench'), mode);
    const expectedInk = mode === 'experiment' ? 'rgb(189, 200, 210)' : 'rgb(32, 52, 77)';
    assert.equal(await page.locator('.shell').evaluate(element => getComputedStyle(element).color), expectedInk);
    assert(await page.evaluate(() => document.fonts.check('20px Explora')));
    assert.equal(await page.locator('.brand-name').evaluate(element => getComputedStyle(element).rowGap), '12px');
    assert.equal(await page.locator('.brand-name').evaluate(element => getComputedStyle(element).fontWeight), '700');
    for (const selector of ['.sidebar', '.shell > main', '.shell > main > header']) {
      assert.match(await page.locator(selector).evaluate(element => getComputedStyle(element).backgroundImage), /linear-gradient/);
    }
    await page.screenshot({ path: join(report, `${mode}-home.png`) });
    const header = page.locator('.shell > main > header');
    for (const width of [1440, 1024, 375]) {
      await page.setViewportSize({ width, height: 900 });
      await page.evaluate(() => window.scrollTo(0, 0));
      const initial = await header.boundingBox();
      await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
      await page.waitForTimeout(80);
      const scrolled = await header.boundingBox();
      assert.equal(Math.round(scrolled.y), 0, `Header remains at top in ${mode}/${width}`);
      assert(await page.evaluate(() => window.scrollY > 0));
      assert(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${mode}/${width} horizontal overflow`);
      cases.push(`${mode}/${width} gradients and stationary header, initial y=${initial.y}`);
    }
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.locator('.sidebar nav').getByRole('button', { name: '项目设置' }).click();
    await page.getByRole('heading', { name: '工作台信息', exact: true }).waitFor();
    assert.equal(await page.evaluate(() => window.scrollY), 0);
    const input = page.locator('.content input').first();
    assert.equal(await input.evaluate(element => getComputedStyle(element).color), expectedInk);
    const details = page.getByRole('button', { name: '技术详情', exact: true }).first();
    assert.match(await details.getAttribute('class'), /secondary-button/);
    assert.match(await details.evaluate(element => getComputedStyle(element).backgroundImage), /linear-gradient/);
    await page.screenshot({ path: join(report, `${mode}-settings.png`) });
    await page.locator('.brand').click();
    const plan = page.getByRole('button', { name: '＋ 新建计划', exact: true });
    await plan.click();
    await page.getByRole('heading', { name: '接入管理', exact: true }).waitFor();
    await page.waitForTimeout(600);
    assert.equal(Math.round((await header.boundingBox()).y), 0);
    await page.screenshot({ path: join(report, `${mode}-connections.png`) });
    await page.getByRole('button', { name: '退出工作台', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '确认退出工作台？' });
    await dialog.waitFor();
    assert.equal(await dialog.evaluate(element => getComputedStyle(element).color), expectedInk);
    const back = dialog.getByRole('button', { name: '返回工作台', exact: true });
    assert.equal(await back.evaluate(element => getComputedStyle(element).transitionDuration), '0.15s');
    await back.hover();
    await page.mouse.down();
    await page.waitForTimeout(180);
    assert.equal(await back.evaluate(element => getComputedStyle(element, '::after').opacity), '1');
    await page.mouse.up();
    await dialog.waitFor({ state: 'hidden' });
    await page.emulateMedia({ reducedMotion: 'reduce' });
    const primary = page.locator('.content button.primary').first();
    assert.equal(await primary.evaluate(element => getComputedStyle(element).transitionDuration), '0s');
    await page.getByRole('button', { name: '退出工作台', exact: true }).click();
    await dialog.waitFor();
    assert.equal(await back.evaluate(element => getComputedStyle(element).transitionDuration), '0s');
    assert.equal(await back.evaluate(element => getComputedStyle(element, '::after').transitionDuration), '0s');
    await back.hover();
    await page.mouse.down();
    assert.equal(await back.evaluate(element => getComputedStyle(element).transform), 'none');
    await page.mouse.up();
    await dialog.waitFor({ state: 'hidden' });
    await page.locator('.brand').focus();
    await page.keyboard.press('Tab');
    assert.notEqual(await page.evaluate(() => getComputedStyle(document.activeElement).outlineStyle), 'none');
    assert(!await page.evaluate(() => window.fixtureCalls.includes('core_submit')));
    cases.push(`${mode} navigation, native identity, bundled font, forms, portal dialog, 150ms press and reduced motion`);
    await page.close();
  }
  assert.deepEqual(errors, []);
  assert.equal(externalRequests, 0);
  await writeFile(join(report, 'result.json'), JSON.stringify({ passed: true, cases, errors, externalRequests }, null, 2));
  console.log(`${cases.length} theme checks passed. ${report}`);
} finally {
  await browser?.close();
}
