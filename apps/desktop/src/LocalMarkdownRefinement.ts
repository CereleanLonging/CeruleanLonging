// In-process, offline document tool. No credentials, IPC, filesystem or network API.
export const localMarkdownTool = { id: "local-markdown-v1", name: "本地 Markdown 提炼工具" } as const;
export const localRefinementLabels = {
  objective: "目标", task: "任务", acceptance: "验收标准", constraint: "约束",
  question: "待澄清问题", context: "背景与补充",
} as const;
type Kind = keyof typeof localRefinementLabels;
export type LocalPoint = { kind: Kind; text: string; quote: string; startLine: number; endLine: number };
export type LocalDocument = { title: string; points: LocalPoint[]; lines: number; tool: typeof localMarkdownTool.id };
type Heading = { level: number; text: string; passive: boolean; kind?: Kind };

const passiveHeading = /^(?:已完成|历史|(?:需求|代码|配置|功能|用法)?示例|例子|参考|引用|更新记录|completed|history|examples?|references?|changelog)(?:$|\b|[：:\s]|[\u3400-\u9fff])/i;
const cleanHeading = (text: string) => text.replace(/\s+#+\s*$/, "").replace(/^(?:\*\*|__)(.*)(?:\*\*|__)$/, "$1").trim();
function category(text: string): Kind | undefined {
  const value = cleanHeading(text).replace(/^(?:\d+[.)、]\s*|[一二三四五六七八九十]+[、.．]\s*)/, "");
  if (/^(?:验收|验证标准|通过标准|acceptance\b|definition of done\b|validation\b)/i.test(value)) return "acceptance";
  if (/^(?:约束|限制|边界|禁止|不得|不能|必须|不允许|constraints?\b|limitations?\b|must\b|shall\b|do not\b)/i.test(value)) return "constraint";
  if (/^(?:待澄清|待确认|疑问|问题|questions?\b|open issues?\b|clarifications?\b)/i.test(value)) return "question";
  if (/^(?:目标|目的|愿景|objectives?\b|goals?\b|aims?\b)/i.test(value)) return "objective";
  if (/^(?:任务|待办|实施步骤|功能需求|需求|开发任务|交付|tasks?\b|todo\b|requirements?\b|implementation\b|deliverables?\b)/i.test(value)) return "task";
  if (/^(?:背景|补充|说明|context\b|background\b|notes?\b)/i.test(value)) return "context";
  return undefined;
}

/** Structural extraction, not semantic inference. Every output retains a verbatim source block. */
export async function refineLocalMarkdown(
  source: string,
  filename: string,
  active: () => boolean = () => true,
  progress: (done: number, total: number) => void = () => {},
): Promise<LocalDocument> {
  const points: LocalPoint[] = [], headings: Heading[] = [];
  let title = filename.replace(/\.md$/i, ""), titleFound = false;
  let pending: string[] = [], firstLine = 0, listIndent: number | undefined;
  let fence: { char: string; count: number } | undefined;
  let blockIsCode = false;
  const current = () => { if (!active()) throw Error("文档或草稿已变化，已停止本地提炼。"); };
  function flush() {
    if (!pending.length) return;
    const quote = pending.join("\n"), raw = quote.trim();
    const body = blockIsCode ? raw : raw.replace(/^(?:[-+*]|\d+[.)、])\s+/, "");
    const passive = blockIsCode || /^>/.test(raw) || /^\[[xX]\]\s/.test(body) || headings.some(h => h.passive);
    const kind = passive ? "context" : category(body) ?? [...headings].reverse().find(h => h.kind)?.kind ?? (/^\[ \]\s/.test(body) ? "task" : "context");
    const section = headings.map(h => h.text).join(" / ");
    points.push({ kind, text: `${section ? `【${section}】\n` : ""}${body}`, quote, startLine: firstLine + 1, endLine: firstLine + pending.length });
    pending = []; listIndent = undefined; blockIsCode = false;
  }
  function heading(level: number, text: string, quote: string, line: number) {
    flush();
    while (headings.length && headings[headings.length - 1].level >= level) headings.pop();
    const label = cleanHeading(text);
    headings.push({ level, text: label, passive: passiveHeading.test(label), kind: category(label) });
    if (!titleFound) { title = label; titleFound = true; }
    // A heading may itself be a requirement. Preserve it even if its section is empty.
    points.push({ kind: "context", text: `章节：${headings.map(h => h.text).join(" / ")}`, quote, startLine: line + 1, endLine: line + quote.split("\n").length });
  }
  let cursor = 0, index = 0, yieldedAt = 0, yieldedTime = performance.now();
  progress(0, source.length);
  for (; cursor <= source.length; index++) {
    if (index % 256 === 0 || cursor - yieldedAt >= 65536) {
      current(); progress(cursor, source.length);
      if (index === 0 || performance.now() - yieldedTime >= 8) {
        await new Promise<void>(resolve => setTimeout(resolve, 0));
        yieldedTime = performance.now();
      }
      current();
      yieldedAt = cursor;
    }
    const end = source.indexOf("\n", cursor);
    const line = source.slice(cursor, end < 0 ? source.length : end);
    cursor = end < 0 ? source.length + 1 : end + 1;
    if (fence) {
      pending.push(line);
      const end = /^ {0,3}(`{3,}|~{3,})\s*$/.exec(line);
      if (end && end[1][0] === fence.char && end[1].length >= fence.count) { fence = undefined; flush(); }
      continue;
    }
    const start = /^ {0,3}(`{3,}|~{3,})/.exec(line);
    if (start) { flush(); firstLine = index; pending = [line]; blockIsCode = true; fence = { char: start[1][0], count: start[1].length }; continue; }
    const atx = /^ {0,3}(#{1,6})\s+(.+)$/.exec(line);
    if (atx) { heading(atx[1].length, atx[2], line, index); continue; }
    const nextEnd = source.indexOf("\n", cursor);
    const nextLine = source.slice(cursor, nextEnd < 0 ? source.length : nextEnd);
    if (line.trim() && cursor <= source.length && /^ {0,3}(?:=+|-+)\s*$/.test(nextLine) && !/^\s*(?:[-+*]|\d+[.)、])\s+/.test(line) && !pending.length) {
      heading(nextLine.trim()[0] === "=" ? 1 : 2, line, `${line}\n${nextLine}`, index);
      cursor = nextEnd < 0 ? source.length + 1 : nextEnd + 1;
      index++; continue;
    }
    if (!line.trim() || /^ {0,3}(?:(?:\*\s*){3,}|(?:-\s*){3,}|(?:_\s*){3,})$/.test(line)) { flush(); continue; }
    const list = /^(\s*)(?:[-+*]|\d+[.)、])\s+/.exec(line);
    if (list && (listIndent === undefined || list[1].length <= listIndent)) flush();
    if (!pending.length) { firstLine = index; listIndent = list ? list[1].length : undefined; }
    pending.push(line);
  }
  current(); flush(); progress(source.length, source.length);
  if (!points.length) throw Error("文档没有可整理的正文，请核对原文。");
  return { title, points, lines: index, tool: localMarkdownTool.id };
}

export async function localRefinementText(document: LocalDocument,
  active: () => boolean = () => true,
  progress: (done: number, total: number) => void = () => {},
) {
  const sections: string[] = [], tasks: string[] = [];
  const groups = new Map<Kind, LocalPoint[]>();
  let yieldedTime = performance.now();
  for (let index = 0; index < document.points.length; index++) {
    if (index % 256 === 0) {
      if (!active()) throw Error("文档或草稿已变化，已停止本地提炼。");
      progress(index, document.points.length * 2);
      if (index === 0 || performance.now() - yieldedTime >= 8) {
        await new Promise<void>(resolve => setTimeout(resolve, 0));
        yieldedTime = performance.now();
      }
    }
    const point = document.points[index];
    const group = groups.get(point.kind) ?? [];
    group.push(point);
    groups.set(point.kind, group);
  }
  let merged = 0;
  for (const [kind, label] of Object.entries(localRefinementLabels)) {
    const points = groups.get(kind as Kind);
    if (points?.length) {
      const text: string[] = [];
      let previousSection = "";
      for (let index = 0; index < points.length; index++) {
        if (index % 256 === 0) {
          if (!active()) throw Error("文档或草稿已变化，已停止本地提炼。");
          progress(document.points.length + merged, document.points.length * 2);
          if (index === 0 || performance.now() - yieldedTime >= 8) {
            await new Promise<void>(resolve => setTimeout(resolve, 0));
            yieldedTime = performance.now();
          }
        }
        const point = points[index];
        const section = /^【[^\n]*】\n/.exec(point.text)?.[0] ?? "";
        text.push(section && section === previousSection ? point.text.slice(section.length) : point.text);
        previousSection = section;
        // One push per item avoids the argument-count limit on large documents.
        tasks.push(`${label}：${point.text}`);
        merged++;
      }
      sections.push(`${label}\n${text.join("\n\n")}`);
    }
  }
  if (!active()) throw Error("文档或草稿已变化，已停止本地提炼。");
  progress(document.points.length * 2, document.points.length * 2);
  return { title: document.title, text: [document.title, ...sections].join("\n\n"), tasks };
}
