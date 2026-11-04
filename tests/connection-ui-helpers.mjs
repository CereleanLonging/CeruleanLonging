const labels = {api:'API 连接', key:'Key 管理', desktop:'桌面端接入', cli:'CLI 接入'};

export async function openConnectionMethod(page, role, method) {
  const region = page.getByRole('region', {name:role, exact:true});
  const summaries = region.locator(':scope > details > summary');
  const summary = summaries.filter({hasText:new RegExp('^'+labels[method]+'$')});
  if (!await summary.evaluate(element => element.parentElement.open)) await summary.click();
  if (role === '主指挥' && method !== 'key' && await page.locator('.project-connections').count()) {
    await page.waitForFunction(expected => {
      const source = document.querySelector('#planning-chief-source');
      return source && (source.value === 'codexSession') === (expected === 'desktop');
    }, method);
  }
  if (role === '主指挥' && !await page.locator('.project-connections').count()) {
    await summary.locator('..').locator(method === 'desktop' ? '.integration-note' : '.integration-entry').waitFor();
  }
}

export async function openConfiguredConnections(page) {
  if (!await page.locator('.project-connections').count()) return;
  const source = await page.locator('#planning-chief-source').inputValue();
  await openConnectionMethod(page, '主指挥', source === 'codexSession' ? 'desktop' : 'api');
  await openConnectionMethod(page, '副指挥', 'api');
  const adapter = await page.locator('#planning-adapter').inputValue();
  const method = ['codex','omp'].includes(adapter) ? 'cli' : ['qoder','workbuddy'].includes(adapter) ? 'desktop' : 'api';
  await openConnectionMethod(page, '执行端', method);
}
