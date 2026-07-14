/**
 * 命令面板（Cmd+K）数据层（U1 / KTD2 / KTD3）。
 *
 * 零依赖模糊匹配：项目 devDependencies 刻意极简，不引第三方 fuzzy 库。
 * 面板是**开发者加速器**（ideation #7 护栏：可见导航承载，面板非唯一入口），
 * 故命令集小、匹配算法只需子序列 + 连续/词首加权即够用。纯函数，独立可测。
 */

/** 一条可执行命令。`run` 让导航与全局动作同构，也为后续路由动作命令预留扩展点（KTD3）。 */
export type Command = {
  id: string;
  title: string;
  /** 中英/拼音别名，供 title 为中文时命中（如 "clean"/"qingli" → "清理"）。 */
  keywords?: string[];
  run: () => void;
};

/**
 * 子序列匹配打分：needle 的每个字符须按序出现在 haystack 中，否则 0（不命中）。
 * 命中则按「连续命中」「词首命中」加权，使更贴合的命令排在前。均已小写。
 */
function scoreMatch(haystack: string, needle: string): number {
  if (needle === "") return 1;
  let score = 0;
  let hayIdx = 0;
  let prevMatch = -1;
  for (const ch of needle) {
    const found = haystack.indexOf(ch, hayIdx);
    if (found === -1) return 0; // 有字符无法按序命中 → 整体不命中
    score += 1; // 基础命中分
    if (found === prevMatch + 1) score += 5; // 连续命中加权
    if (found === 0) score += 10; // 词首命中加权
    prevMatch = found;
    hayIdx = found + 1;
  }
  return score;
}

/** 对单条命令取 title 与所有 keyword 的最高分。 */
function commandScore(command: Command, needle: string): number {
  const haystacks = [command.title, ...(command.keywords ?? [])];
  let best = 0;
  for (const h of haystacks) {
    const s = scoreMatch(h.toLowerCase(), needle);
    if (s > best) best = s;
  }
  return best;
}

/**
 * 按 query 过滤并排序命令。
 * - query 为空（或纯空白）：原序返回全部。
 * - 否则：剔除不命中项，按分降序；同分保持输入相对顺序（稳定）。
 */
export function fuzzyFilter(commands: Command[], query: string): Command[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return [...commands];

  return commands
    .map((command, idx) => ({ command, idx, score: commandScore(command, needle) }))
    .filter((entry) => entry.score > 0)
    .sort((a, b) => b.score - a.score || a.idx - b.idx) // 同分按原序（显式，不依赖 sort 稳定性）
    .map((entry) => entry.command);
}
