/**
 * clean_rules 的已知分类，作为 StreamingList 预印占位行 / SummaryHeader 图例的顺序（KTD2）。
 *
 * **必须与 `crates/core/src/clean_rules.toml` 的 category 集合一致**：漏掉某分类会导致其行在
 * 扫描中途才插入（行新增＝跳变，破坏防跳变）。`categories.test.ts` 直接读取该 TOML 做 parity
 * 断言——新增品类时测试会红，提示同步此处（取代仅靠注释的弱守卫，async-UI review P3）。
 */
export const KNOWN_CATEGORIES = ["系统缓存", "浏览器缓存"] as const;
