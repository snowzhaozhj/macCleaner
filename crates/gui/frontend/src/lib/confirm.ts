/**
 * type-to-confirm 门槛（DESIGN.md §6 · TUI reporter.rs 同源）。
 * Risky 项唯一删除通道：用户必须逐字输入 token 才启用删除按钮。
 * Enter 不绑定提交——必须点已启用按钮。
 */
export const CONFIRM_TOKEN = "delete";

/** 大小写不敏感、去首尾空格后精确等于 token 才算确认。 */
export function isConfirmed(input: string): boolean {
  return input.trim().toLowerCase() === CONFIRM_TOKEN;
}
