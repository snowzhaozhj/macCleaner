/**
 * 撤销 toast 的单实例状态（U5 / R11）。
 *
 * 「单实例」＝任何新一次删除都**替换**当前 toast，绝不堆叠成多条。用递增 `seq` 表达
 * 「这是新一次」——UI 可 key 于 seq 强制重挂载以重置自动消失计时/重播进出动画。
 * 纯逻辑便于单测（Verification Contract：undo toast 单实例逻辑）。
 */
export type ToastState = { count: number; freed: number; seq: number } | null;

/** 生成/替换 toast（单实例）：始终返回单个对象，seq 在前值基础上 +1。 */
export function nextToast(
  prev: ToastState,
  count: number,
  freed: number,
): ToastState {
  return { count, freed, seq: (prev?.seq ?? 0) + 1 };
}

/** 关闭 toast。 */
export function dismissToast(): ToastState {
  return null;
}
