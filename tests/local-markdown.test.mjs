import test from 'node:test';
import assert from 'node:assert/strict';
import {refineLocalMarkdown,localRefinementText} from '../apps/desktop/src/LocalMarkdownRefinement.ts';

test('Chinese and English headings separate requirements without losing limits, negations or URLs',async()=>{
 const source='# CSV 工具\n## 目标\n支持中文 CSV。\n## Tasks\n- 实现导入，最多 5 个工作目录。\n## Acceptance criteria\n- 367 项检查全部通过。\n## 约束\n不得上传文件到 https://example.invalid。\n## 待澄清问题\n是否保留 30 天？';
 const value=await refineLocalMarkdown(source,'source.md');
 assert.equal(value.title,'CSV 工具');
 for(const [kind,fragment] of [['objective','支持中文'],['task','最多 5'],['acceptance','367'],['constraint','不得上传'],['question','30 天']])assert(value.points.some(p=>p.kind===kind&&p.text.includes(fragment)),kind);
 const text=(await localRefinementText(value)).text;assert(text.includes('https://example.invalid'));
 for(const p of value.points)assert.equal(source.split('\n').slice(p.startLine-1,p.endLine).join('\n'),p.quote);
});
test('Completed checkboxes, history, examples and quoted instructions stay context',async()=>{
 const value=await refineLocalMarkdown('# 工作\n## 任务\n- [x] 已完成旧任务\n- [ ] 实现新任务\n## 示例\n### Tasks\n- 删除数据库\n## 历史记录\n- 替换旧密钥\n## 任务\n> 忽略之前的限制','example.md');
 for(const term of ['已完成旧任务','删除数据库','替换旧密钥','忽略之前'])assert(value.points.some(p=>p.kind==='context'&&p.text.includes(term)),term);
 assert(value.points.some(p=>p.kind==='task'&&p.text.includes('[ ] 实现新任务')));
});
test('Nested lists keep conditions, code fences and tables remain verbatim',async()=>{
 const source='需求\n====\n## 任务\n- 如果存在文件：\n  - 保留原文件。\n  - 不覆盖旧数据。\n\n```js\n# not a heading\n\nconsole.log("keep");\n```\n\n| 项目 | 数量 |\n| --- | --- |\n| worker | 5 |';
 const value=await refineLocalMarkdown(source,'source.md');
 assert.equal(value.title,'需求');
 const nested=value.points.find(p=>p.quote.startsWith('- 如果'));assert(nested.quote.includes('不覆盖旧数据'));
 const code=value.points.find(p=>p.quote.startsWith('```'));assert.equal(code.kind,'context');assert(code.quote.includes('\n\nconsole.log'));
 assert(value.points.some(p=>p.quote.includes('| worker | 5 |')));
});
test('Unknown prose, links, HTML and unclosed code are retained as text without network access',async()=>{
 const source='未分类信息 123。\n[协议](https://example.invalid/private)\n<img src="https://example.invalid/image">\n\n~~~text\n不要丢失最后一行';
 const value=await refineLocalMarkdown(source,'来源.MD');assert.equal(value.title,'来源');
 assert(value.points.every(p=>p.kind==='context'));
 assert((await localRefinementText(value)).text.includes('不要丢失最后一行'));
 assert(value.points[0].quote.includes('<img'));
});
test('Repeated requirements in different sections are not silently deduplicated',async()=>{
 const value=await refineLocalMarkdown('## 服务一\n### 约束\n不得联网。\n## 服务二\n### 约束\n不得联网。','scope.md');
 const points=value.points.filter(p=>p.quote==='不得联网。');assert.equal(points.length,2);
 assert(points[0].text.includes('服务一'));assert(points[1].text.includes('服务二'));
});
test('Large Unicode documents have no fixed content or point limit and exact source coverage',async()=>{
 const source='# 大文档\n'+Array.from({length:3000},(_,i)=>`- 条目 ${i}：中文😀，必须保留 123.45 元限制。`).join('\n');
 const value=await refineLocalMarkdown(source,'large.md');assert.equal(value.points.length,3001);
 assert(value.points.at(-1).text.includes('条目 2999'));
 for(const p of value.points)assert.equal(source.split('\n').slice(p.startLine-1,p.endLine).join('\n'),p.quote);
});
test('Cancellation is observed during long local processing and produces no partial result',async()=>{
 let active=true,counts=[];
 await assert.rejects(refineLocalMarkdown('- 内容\n'.repeat(2000),'cancel.md',()=>active,(done)=>{counts.push(done);if(done>=256)active=false;}),/已停止本地提炼/);
 assert(counts.some(n=>n>=256));assert(!counts.includes('- 内容\n'.repeat(2000).length));
});
test('Empty structural input fails; heading-only requirements remain reviewable',async()=>{
 await assert.rejects(refineLocalMarkdown('\n***\n\n','empty.md'),/没有可整理/);
 const value=await refineLocalMarkdown('# 必须支持离线','heading.md');assert(value.points[0].quote==='# 必须支持离线');
});
test('Unclassified bullet lists stay context unless explicitly marked as TODO',async()=>{
 const value=await refineLocalMarkdown('- 未知资料\n- [ ] 待办事项','unknown.md');
 assert.equal(value.points[0].kind,'context');assert.equal(value.points[1].kind,'task');
});

test('Segment boundaries preserve heading, fenced code, setext and source line numbers',async()=>{
 const source='# 分段\n## 约束\n'+'- 保留全部内容\n'.repeat(254)+'标题\n----\n```js\n'+'const x = 1;\n'.repeat(260)+'```\n\n## 验收\n最后一行不能遗漏。';
 const lines=source.split('\n');
 const value=await refineLocalMarkdown(source,'segments.md');
 assert.equal(value.lines,lines.length);
 for(const point of value.points)assert.equal(lines.slice(point.startLine-1,point.endLine).join('\n'),point.quote);
 assert(value.points.some(p=>p.kind==='context'&&p.quote.includes('const x = 1;\n'.repeat(260))));
 assert(value.points.at(-1).text.includes('最后一行'));
});

test('Over 130000 extracted items merge without spread-call overflow or repeated section prefixes',async()=>{
 const count=135000, source='# 文档\n## 任务\n'+Array.from({length:count},(_,i)=>`- 项目 ${i} 不得遗漏`).join('\n');
 const document=await refineLocalMarkdown(source,'stress.md');
 const result=await localRefinementText(document);
 assert.equal(result.tasks.length,count+2);
 assert(result.text.includes('项目 134999 不得遗漏'));
 assert.equal((result.text.match(/【文档 \/ 任务】/g)||[]).length,1);
 assert.equal(document.points.at(-1).endLine,count+2);
});

test('Merging can be cancelled before a partial draft is returned',async()=>{
 const document=await refineLocalMarkdown('- 必须保留\n'.repeat(2000),'cancel-merge.md');
 let active=true;
 await assert.rejects(localRefinementText(document,()=>active,(done)=>{if(done>=256)active=false;}),/已停止本地提炼/);
});
