/**
 * 命令面板路由内动作命令的注册表（KTD1）。
 *
 * 模块级 `$state`：当前挂载路由把「此刻可执行的动作命令」写进来，`App.svelte` 读它并与
 * 静态命令（4 导航 + 2 全局动作）合并给面板。**单路由不变量**——tab 切换会卸载旧路由、
 * 挂载新路由，故任一时刻注册表只含一个路由的命令；`setRouteCommands` 是覆盖式（非追加），
 * `clearRouteCommands` 由路由 `$effect` cleanup 在卸载时调用。
 *
 * 为何用共享 rune 模块而非 `setContext`/`getContext`：仅 App + 当前路由两方共享，context 需
 * provider 包裹 + 三处样板；`.svelte.ts` 顶层 `$state` 更直接（KTD1）。
 *
 * 反应式契约：`routeCommands()` 在 `$derived`/`$effect` 等反应式作用域内读取时，写入
 * `setRouteCommands`/`clearRouteCommands` 会触发依赖方重算（面板 `filtered` 自动重排）。
 *
 * 单路由不变量依赖 Svelte「先同步销毁旧路由（cleanup→clear）、后运行新路由 effect（set）」的顺序
 * （App 的 `{#if tab}/{:else if}` 无过渡指令，成立）。**若未来给路由级切换包 view-transition/outro，
 * 销毁会延后到新路由挂载之后，clear 反会清空新命令**——届时须改为按路由实例 token 归属清空
 * （评审 julik-frontend-races）。
 */
import type { Command } from "./palette";

let _routeCommands = $state<Command[]>([]);

/** 当前路由注册的命令（反应式读）。 */
export function routeCommands(): Command[] {
  return _routeCommands;
}

/** 覆盖式写入当前路由命令集（非追加——单路由不变量）。 */
export function setRouteCommands(commands: Command[]): void {
  _routeCommands = commands;
}

/** 清空（路由卸载时由 `$effect` cleanup 调用）。 */
export function clearRouteCommands(): void {
  _routeCommands = [];
}

/**
 * 路由动作命令注册的生命周期编排（供路由在 `<script>` 顶层同步调用）。
 * 挂载期注册、随 `getCommands` 依赖变化更新、卸载时自动清空——把四路由逐字相同的
 * `$effect(注册/清空)` 样板收敛到一处。
 *
 * **必须传 getter（`() => cmds`）而非值**：读取须发生在 `$effect` 内才建立反应式依赖，
 * 传值会让命令集冻结在首次快照、不再随相位/选择态增删（违背 KTD2）。
 * 只能在组件初始化期（`<script>` 顶层）同步调用——事件处理器/`await` 之后调用会 `effect_orphan`。
 */
export function registerRouteCommands(getCommands: () => Command[]): void {
  $effect(() => {
    setRouteCommands(getCommands());
    return clearRouteCommands;
  });
}
