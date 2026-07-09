/**
 * IPC 契约守卫（U3 / R4 / SC4）——信任锚。
 *
 * 静态比对三方，把「按钮静默失效」这一类故障在编译期挡下：
 *  1. Rust 注册表：`crates/gui/src/lib.rs` 的 `generate_handler![...]`（命令集合）。
 *  2. Rust 命令签名：`crates/gui/src/commands/*.rs` 的 `#[tauri::command] fn`（参数名，snake_case）。
 *  3. 前端调用：`src/lib/ipc.ts` 的 `invoke("name", { ...args })`（命令名 + 参数键，camelCase）。
 *
 * 断言：命令集合三方一致；前端每个参数（camel→snake）都存在于对应 Rust 签名。
 * Tauri v2 默认把 JS camelCase 参数转为 Rust snake_case——本守卫把这层约定钉死，改名/改参即红。
 *
 * 跑在 vitest（Node，fs 解析），非 Playwright。
 */
import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

const libRs = fileURLToPath(new URL("../../src/lib.rs", import.meta.url));
const commandsDir = fileURLToPath(new URL("../../src/commands/", import.meta.url));
const ipcTs = fileURLToPath(new URL("../src/lib/ipc.ts", import.meta.url));

// ---- 解析器（纯函数，便于负向自证）----

/** 从 generate_handler![...] 提取命令名（取 `commands::mod::name` 的末段）。 */
export function parseRegisteredCommands(libSrc: string): string[] {
  const m = libSrc.match(/generate_handler!\s*\[([\s\S]*?)\]/);
  if (!m) return [];
  return m[1]
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean)
    .map((path) => path.split("::").pop()!.trim());
}

/** 从 commands/*.rs 提取每个 #[tauri::command] fn 的参数名（排除 app/State），键为命令名。 */
export function parseRustCommandArgs(sources: string[]): Map<string, string[]> {
  const out = new Map<string, string[]>();
  const fnRe = /#\[tauri::command\][\s\S]*?fn\s+(\w+)\s*\(([\s\S]*?)\)\s*(?:->|\{)/g;
  for (const src of sources) {
    let m: RegExpExecArray | null;
    while ((m = fnRe.exec(src)) !== null) {
      const name = m[1];
      const argNames = m[2]
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean)
        .filter((seg) => !/:\s*AppHandle/.test(seg) && !/State\s*</.test(seg))
        .map((seg) => seg.split(":")[0].trim())
        .filter(Boolean);
      out.set(name, argNames);
    }
  }
  return out;
}

/** 从 ipc.ts 提取 invoke 调用：命令名 + 参数键（camelCase，含 shorthand）。 */
export function parseIpcInvocations(ipcSrc: string): Map<string, string[]> {
  const out = new Map<string, string[]>();
  const re = /invoke(?:<[^>]*>)?\(\s*"([^"]+)"\s*(?:,\s*\{([^}]*)\})?\s*\)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(ipcSrc)) !== null) {
    const name = m[1];
    const body = m[2] ?? "";
    const keys = body
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean)
      .map((entry) => entry.split(":")[0].trim()); // `onEvent: channel` → onEvent；`paths` → paths
    out.set(name, keys);
  }
  return out;
}

export function camelToSnake(s: string): string {
  return s.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);
}

/** 命令集合差异（负向自证用）。 */
export function diffCommands(frontend: string[], rust: string[]): { missing: string[]; extra: string[] } {
  const fset = new Set(frontend);
  const rset = new Set(rust);
  return {
    missing: [...rset].filter((c) => !fset.has(c)), // Rust 有、前端没封装
    extra: [...fset].filter((c) => !rset.has(c)), // 前端调了、Rust 没注册（拼错/漏注册）
  };
}

// ---- 读取源 ----

const libSrc = readFileSync(libRs, "utf8");
const commandSrcs = readdirSync(commandsDir)
  .filter((f) => f.endsWith(".rs"))
  .map((f) => readFileSync(commandsDir + f, "utf8"));
const ipcSrc = readFileSync(ipcTs, "utf8");

const registered = parseRegisteredCommands(libSrc);
const rustArgs = parseRustCommandArgs(commandSrcs);
const ipcCalls = parseIpcInvocations(ipcSrc);

describe("IPC 契约守卫（R4）", () => {
  it("Rust 注册表解析出 9 个命令", () => {
    expect(registered.length).toBe(9);
    expect(new Set(registered)).toEqual(
      new Set([
        "scan_clean",
        "clean",
        "cancel_scan",
        "analyze",
        "classify_marked",
        "delete_marked",
        "check_fda",
        "open_fda_settings",
        "open_trash",
      ]),
    );
  });

  it("前端 invoke 命令集合与 Rust 注册表完全相等（无缺失、无多余、无拼错）", () => {
    const { missing, extra } = diffCommands([...ipcCalls.keys()], registered);
    expect(missing).toEqual([]);
    expect(extra).toEqual([]);
  });

  it("前端每个 invoke 参数（camel→snake）都存在于对应 Rust 命令签名", () => {
    const violations: string[] = [];
    for (const [cmd, keys] of ipcCalls) {
      const rust = rustArgs.get(cmd);
      if (!rust) {
        violations.push(`${cmd}: Rust 无同名命令函数`);
        continue;
      }
      for (const key of keys) {
        const snake = camelToSnake(key);
        if (!rust.includes(snake)) {
          violations.push(`${cmd}.${key}→${snake} 不在 Rust 签名 [${rust.join(", ")}]`);
        }
      }
    }
    expect(violations).toEqual([]);
  });

  it("关键映射：clean/delete_marked 的 confirmToken/onEvent 映射到 confirm_token/on_event", () => {
    for (const cmd of ["clean", "delete_marked"]) {
      expect(ipcCalls.get(cmd)).toContain("confirmToken");
      expect(ipcCalls.get(cmd)).toContain("onEvent");
      expect(rustArgs.get(cmd)).toContain("confirm_token");
      expect(rustArgs.get(cmd)).toContain("on_event");
    }
  });

  // 负向自证：守卫真的能红。若前端多调一个未注册命令，diffCommands 必须把它列为 extra。
  it("负向自证：前端多调未注册命令时被判为 extra", () => {
    const { extra } = diffCommands([...registered, "ghost_command"], registered);
    expect(extra).toContain("ghost_command");
  });

  it("负向自证：camel→snake 转换正确", () => {
    expect(camelToSnake("confirmToken")).toBe("confirm_token");
    expect(camelToSnake("onEvent")).toBe("on_event");
    expect(camelToSnake("paths")).toBe("paths");
  });
});
