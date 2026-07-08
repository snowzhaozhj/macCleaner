/**
 * Tauri IPC 边界 mock（U1 / KTD1 / KTD2）。
 *
 * 通过 `page.addInitScript` 在 app bootstrap 前把假的 `window.__TAURI_INTERNALS__` 装入页面：
 * `@tauri-apps/api` v2 的 `invoke` 经 `__TAURI_INTERNALS__.invoke(cmd, args)` 路由，`Channel`
 * 构造时调 `__TAURI_INTERNALS__.transformCallback`——两者都被我们接管。流式命令直接调
 * `args.onEvent.onmessage(evt)` 回放事件流（app 在 invoke 前已 `channel.onmessage = onEvent`，
 * 故该 handler 已就位）。真实后端/真实删除一概不走（本轮范围）。
 *
 * 设计要点：
 * - handler spec 是**纯数据**（events/result/error/pending），可经 addInitScript 序列化注入。
 * - 每次 invoke 记入 `window.__TAURI_MOCK__.calls`（Channel 参数脱敏为 "[Channel]"），供测试
 *   断言「按钮触发了正确命令 + 正确参数」（R3）。
 * - **未注册命令 reject 并带诊断**（计划 U1 契约）：漏 mock 的调用暴露为清晰失败，而非静默 undefined。
 * - 协作式取消：`pending:true` 的 spec 返回悬挂 Promise；`cancel_scan` 被调用时 reject 所有悬挂项，
 *   模拟真实 scan_clean/analyze 在取消时抛错（Clean/Analyze 组件 catch 后回到 results/ready）。
 */
import type { Page } from "@playwright/test";

/** 一条命令的 mock 行为（纯数据，可序列化注入）。 */
export type HandlerSpec = {
  /** 依次经 Channel.onmessage 回放的事件流（ProgressEvent / AnalyzeEvent 形状）。 */
  events?: unknown[];
  /** invoke resolve 的最终值（如 ScanResult / CleanReport / DirNode / FdaStatus）。 */
  result?: unknown;
  /** 置位则 invoke reject（Error(message)），模拟命令层失败（区别于流内的 Error 事件）。 */
  error?: string;
  /** 置位则 invoke 返回悬挂 Promise，直到 cancel_scan 到来才 reject（取消测试用）。 */
  pending?: boolean;
};

export type Handlers = Record<string, HandlerSpec>;

/** 一条被记录的 invoke 调用（Channel 参数已脱敏）。 */
export type RecordedCall = { cmd: string; args: Record<string, unknown> };

/**
 * 安装 mock。必须在 `page.goto` 之前调用（addInitScript 在页面脚本前运行）。
 * `handlers` 应含被测流程启动即会触发的命令默认值（如 check_fda），否则 app 挂载即报未注册。
 */
export async function installTauriMock(page: Page, handlers: Handlers): Promise<void> {
  await page.addInitScript((initial: Handlers) => {
    const w = window as unknown as {
      __TAURI_MOCK__: {
        handlers: Handlers;
        calls: RecordedCall[];
        pending: { resolve: (v: unknown) => void; reject: (e: unknown) => void }[];
      };
      __TAURI_INTERNALS__: unknown;
    };
    const mock = { handlers: initial || {}, calls: [] as RecordedCall[], pending: [] as { resolve: (v: unknown) => void; reject: (e: unknown) => void }[] };
    w.__TAURI_MOCK__ = mock;

    let cbId = 0;
    w.__TAURI_INTERNALS__ = {
      // Channel 构造会调它拿一个回调 id；我们用不到真实回传（直接驱动 onmessage），返回自增 id 即可。
      transformCallback(_cb: unknown) {
        cbId += 1;
        return cbId;
      },
      async invoke(cmd: string, args: Record<string, unknown> | undefined) {
        const a = args || {};
        const recorded: Record<string, unknown> = {};
        for (const k of Object.keys(a)) {
          const v = a[k];
          const isChannel = k === "onEvent" || (!!v && typeof v === "object" && "onmessage" in (v as object));
          recorded[k] = isChannel ? "[Channel]" : v;
        }
        mock.calls.push({ cmd, args: recorded });

        const spec = mock.handlers[cmd];
        if (!spec) throw new Error(`Unmocked command: ${cmd}`);

        if (spec.events) {
          const ch = a.onEvent as { onmessage?: (e: unknown) => void } | undefined;
          if (ch && typeof ch.onmessage === "function") {
            for (const e of spec.events) ch.onmessage(e);
          }
        }

        // 取消：reject 所有悬挂的 pending 操作（模拟协作式取消致 scan/analyze 抛错）。
        if (cmd === "cancel_scan") {
          const waiting = mock.pending.splice(0);
          for (const p of waiting) p.reject(new Error("cancelled"));
        }

        if (spec.error) throw new Error(spec.error);
        if (spec.pending) {
          return await new Promise((resolve, reject) => mock.pending.push({ resolve, reject }));
        }
        return spec.result;
      },
    };
  }, handlers);
}

/** 读取迄今所有被记录的 invoke 调用。 */
export async function getCalls(page: Page): Promise<RecordedCall[]> {
  return page.evaluate(() => {
    const w = window as unknown as { __TAURI_MOCK__?: { calls: RecordedCall[] } };
    return w.__TAURI_MOCK__?.calls ?? [];
  });
}

/** 某命令最近一次调用（无则 null）。 */
export async function lastCall(page: Page, cmd: string): Promise<RecordedCall | null> {
  const calls = await getCalls(page);
  for (let i = calls.length - 1; i >= 0; i -= 1) {
    if (calls[i].cmd === cmd) return calls[i];
  }
  return null;
}

/** 某命令的全部调用。 */
export async function callsFor(page: Page, cmd: string): Promise<RecordedCall[]> {
  return (await getCalls(page)).filter((c) => c.cmd === cmd);
}
