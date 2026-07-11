<script lang="ts">
  /**
   * Analyze 单路径的只读审查面孔。
   *
   * 证据和请求令牌都封装在 keyed 组件实例内：折叠只隐藏、不丢缓存；路由导航或重扫
   * 卸载实例后，旧 Promise 即使完成也不能写入新实例。这里的结果绝不参与删除确认。
   */
  import { classifyMarked, revealInFinder, type PathSafety } from "./ipc";
  import Safety from "./Safety.svelte";
  import EvidenceCard from "./EvidenceCard.svelte";
  import CopyButton from "./CopyButton.svelte";

  let {
    path,
    panelId,
    expanded,
  }: {
    path: string;
    panelId: string;
    expanded: boolean;
  } = $props();

  const fallback: Omit<PathSafety, "path"> = {
    safety: "Risky",
    impact: "未匹配内置清理规则；无法确认此路径可安全删除，可能包含不可再生的用户数据或应用状态",
    recovery: "请先核对路径内容；若仍在废纸篓，可移回原处，清空后可能无法恢复",
  };

  type EvidenceState = "idle" | "loading" | "ready" | "fallback";
  let evidencePhase = $state<EvidenceState>("idle");
  let evidence = $state<Omit<PathSafety, "path"> | null>(null);
  let evidenceError = $state<string | null>(null);
  let finderError = $state<string | null>(null);
  let requestToken = 0;

  const isUnknown = $derived(
    evidence?.impact.includes("未匹配任何已知清理规则") ?? false,
  );

  async function loadEvidence() {
    const token = ++requestToken;
    evidencePhase = "loading";
    try {
      const result = await classifyMarked([path]);
      const match = result.find((item) => item.path === path);
      if (!match) throw new Error("分类结果未包含目标路径");
      if (token !== requestToken) return;
      const { path: _path, ...nextEvidence } = match;
      evidence = nextEvidence;
      evidenceError = null;
      evidencePhase = "ready";
    } catch (err) {
      if (token !== requestToken) return;
      evidence = fallback;
      evidenceError = `查询 ${path} 的删除安全证据失败：${String(err)}`;
      evidencePhase = "fallback";
    }
  }

  function retry() {
    // disabled 属性阻止用户在途重复提交；令牌仍防御合成事件和未来调用方造成的并发。
    void loadEvidence();
  }

  async function reveal() {
    finderError = null;
    try {
      await revealInFinder(path);
    } catch (err) {
      finderError = `在 Finder 中显示 ${path} 失败：${String(err)}`;
    }
  }

  $effect(() => {
    if (expanded && evidencePhase === "idle") {
      void loadEvidence();
    }
  });

  $effect(() => {
    return () => {
      requestToken += 1;
    };
  });
</script>

{#if expanded}
  <section class="review" id={panelId} aria-label="{path} 的删除审查">
    <div class="identity">
      <span class="path" title={path}>{path}</span>
      <div class="path-actions">
        <CopyButton text={path} label="复制路径" />
        <button class="finder" onclick={reveal} aria-label="在 Finder 中显示 {path}">
          在 Finder 中显示
        </button>
      </div>
    </div>

    <p class="snapshot">只读审查快照 · 删除前会重新核对安全等级与证据</p>

    {#if evidencePhase === "loading"}
      <p
        class="evidence-status"
        role="status"
        aria-live="polite"
        aria-busy="true"
        aria-label="正在查询 {path} 的删除安全证据"
      >
        正在查询删除安全证据…
      </p>
    {/if}

    {#if evidence}
      <div
        class="evidence"
        role="status"
        aria-live="polite"
        aria-label="{path} 的删除安全证据已就绪"
      >
        <div class="safety-line">
          <span class="evidence-label">删除安全等级</span>
          <Safety safety={evidence.safety} />
          {#if isUnknown || evidencePhase === "fallback"}
            <span class="boundary">未匹配内置清理规则</span>
          {/if}
        </div>
        <EvidenceCard
          impact={evidence.impact}
          recovery={evidence.recovery}
          full
        />
      </div>
    {/if}

    {#if evidenceError}
      <div class="failure" role="alert">
        <span>{evidenceError}</span>
        <button
          class="retry"
          disabled={evidencePhase === "loading"}
          onclick={retry}
          aria-label="重新查询 {path}"
        >
          {evidencePhase === "loading" ? "查询中…" : "重新查询"}
        </button>
      </div>
    {/if}

    {#if finderError}
      <p class="failure finder-error" role="alert">{finderError}</p>
    {/if}
  </section>
{/if}

<style>
  .review {
    min-width: 0;
    margin: var(--sp-1) var(--sp-1) var(--sp-2) calc(15px + var(--sp-3));
    padding: var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius);
    background: var(--surface-raised);
  }
  .identity {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--sp-3);
    min-width: 0;
  }
  .path {
    min-width: 0;
    flex: 1 1 auto;
    font-family: var(--font-mono);
    font-size: 0.85em;
    color: var(--ink-primary);
    overflow-wrap: anywhere;
    user-select: text;
  }
  .path-actions {
    display: flex;
    flex: 0 1 auto;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
  .finder,
  .retry {
    flex: 0 0 auto;
    font-family: var(--font-ui);
    font-size: 0.75em;
    padding: 2px var(--sp-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius);
    background: var(--surface-base);
    color: var(--accent-explore);
    cursor: pointer;
  }
  .finder:hover,
  .retry:hover:not(:disabled) {
    border-color: var(--accent-explore);
  }
  .retry:disabled {
    color: var(--ink-faint);
    cursor: not-allowed;
  }
  .snapshot {
    margin: var(--sp-2) 0;
    color: var(--ink-faint);
    font-size: 0.75em;
  }
  .evidence-status {
    margin: var(--sp-2) 0 0;
    color: var(--ink-muted);
    font-size: 0.8em;
  }
  .evidence {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    min-width: 0;
  }
  .safety-line {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }
  .evidence-label,
  .boundary {
    font-size: 0.78em;
    color: var(--ink-muted);
  }
  .boundary {
    color: var(--state-warning);
  }
  .failure {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    margin-top: var(--sp-2);
    color: var(--state-danger);
    font-size: 0.8em;
    overflow-wrap: anywhere;
  }
  .finder-error {
    display: block;
    margin-bottom: 0;
  }
  @media (max-width: 760px) {
    .identity {
      flex-direction: column;
    }
    .path-actions {
      justify-content: flex-start;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .review,
    .review * {
      scroll-behavior: auto;
      transition: none !important;
      animation: none !important;
    }
  }
</style>
