// Render the production home page with no native bridge or provider connection.
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { build } from 'vite';
import { chromium } from 'playwright-core';

assert(process.env.CERULEAN_CACHE_SESSION, 'Use the managed cache runner');
const report = await mkdtemp(resolve('reports/workflow-map-ui-'));
const bundle = join(process.env.CERULEAN_CACHE_SESSION, 'workflow-map-ui');
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
    lib: { entry, name: 'WorkflowHome', formats: ['iife'], fileName: () => 'ui.js', cssFileName: 'ui' } },
});

const cases = [];
const errors = [];
let externalRequests = 0;
let browser;
try {
  browser = await chromium.launch({
    executablePath: 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true,
  });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/*', route => { externalRequests++; return route.abort(); });
  await page.setContent('<html lang="zh-CN"><meta charset="utf-8"><div id="root"></div></html>');
  await page.addStyleTag({ path: join(bundle, 'ui.css') });
  await page.addScriptTag({ path: join(bundle, 'ui.js') });
  const home = page.getByRole('region', { name: '完整工作流程', exact: true });
  await home.waitFor();
  const image = home.locator('> .workflow-map-viewport img');
  await image.evaluate(element => element.decode());
  assert.equal(await image.evaluate(element => element.naturalWidth), 1180);
  assert.equal(await page.locator('.team-map').count(), 0);
  assert.equal(await page.getByRole('button', { name: '＋ 新建计划', exact: true }).count(), 1);
  const source = await readFile('docs/images/workflow-0.1.48-zh.svg', 'utf8');
  const actualSource = await image.evaluate(async element => (await fetch(element.src)).text());
  assert.equal(actualSource, source);
  cases.push('Production home loads the exact README diagram and retains the plan entry');
  await page.screenshot({ path: join(report, 'home.png'), fullPage: true });

  const open = home.getByRole('button', { name: '放大查看', exact: true });
  await open.click();
  const dialog = page.getByRole('dialog', { name: '完整工作流程', exact: true });
  await dialog.waitFor();
  assert(await dialog.getByRole('button', { name: '关闭流程图' }).evaluate(element => element === document.activeElement));
  const viewport = dialog.getByRole('region');
  await viewport.evaluate(element => { element.scrollTop = element.scrollHeight; });
  assert(await viewport.evaluate(element => element.scrollTop > 500));
  await page.screenshot({ path: join(report, 'enlarged-bottom.png') });
  await dialog.getByRole('button', { name: '关闭流程图' }).click();
  assert.equal(await dialog.count(), 0);
  assert(await open.evaluate(element => element === document.activeElement));
  await open.click();
  await page.keyboard.press('Escape');
  assert.equal(await dialog.count(), 0);
  cases.push('Enlarged view scrolls to acceptance, closes by button and Escape, and returns focus');

  await page.setViewportSize({ width: 1024, height: 768 });
  assert(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth));
  const inline = home.locator('> .workflow-map-viewport');
  assert(await inline.evaluate(element => element.scrollWidth > element.clientWidth));
  await open.click();
  const bounds = await dialog.boundingBox();
  assert(bounds.x >= 0 && bounds.x + bounds.width <= 1024 && bounds.y + bounds.height <= 768);
  await page.screenshot({ path: join(report, 'narrow-dialog.png') });
  await page.keyboard.press('Escape');
  cases.push('Narrow desktop keeps horizontal scrolling inside the figure and fits the modal');

  const readme = await readFile('README.md', 'utf8');
  const overflows = [];
  for (const language of ['zh', 'en']) {
    const filename = `docs/images/workflow-0.1.48-${language}.svg`;
    assert(readme.includes(`](${filename})`));
    const svg = await readFile(filename, 'utf8');
    assert(!/<(?:script|foreignObject)|\bon\w+\s*=|(?:href|url\()\s*["']?https?:/i.test(svg));
    await page.setViewportSize({ width: 1180, height: 1756 });
    await page.setContent(`<html><meta charset="utf-8"><body style="margin:0">${svg}</body></html>`);
    const overflow = await page.locator('svg g text').evaluateAll(elements => elements.flatMap(element => {
      const text = element.getBBox();
      const box = element.parentElement.querySelector('rect').getBBox();
      return text.x < box.x + 6 || text.x + text.width > box.x + box.width - 6
        || text.y < box.y || text.y + text.height > box.y + box.height
        ? [{ text: element.textContent, textBox: { x: text.x, width: text.width }, box: { x: box.x, width: box.width } }] : [];
    }));
    overflows.push(...overflow.map(item => ({ language, ...item })));
    await page.screenshot({ path: join(report, `workflow-${language}.png`), fullPage: true });
  }
  await writeFile(join(report, 'text-overflows.json'), JSON.stringify(overflows, null, 2));
  assert.deepEqual(overflows, []);
  cases.push('Both README translations are self-contained SVG with text contained inside nodes');
  assert.deepEqual(errors, []);
  assert.equal(externalRequests, 0);
  await writeFile(join(report, 'result.json'), JSON.stringify({ status: 'passed', cases, errors, externalRequests }, null, 2));
  console.log(`${cases.length} workflow home checks passed. ${report}`);
} finally {
  await browser?.close();
}
