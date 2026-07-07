<script lang="ts">
  import type { SafetyLevel } from "./ipc";
  import { CONFIRM_TOKEN, isConfirmed } from "./confirm";
  import { formatBytes } from "./format";
  import Safety from "./Safety.svelte";
  import PathText from "./PathText.svelte";
  import EvidenceCard from "./EvidenceCard.svelte";

  export type ConfirmItem = {
    path: string;
    size: number;
    safety?: SafetyLevel;
    impact?: string;
    recovery?: string;
  };

  let {
    items,
    onConfirm,
    onCancel,
  }: {
    items: ConfirmItem[];
    // 回传用户输入的确认口令（无 Risky 时为空串）；调用方转发给后端二次校验。
    onConfirm: (token: string) => void;
    onCancel: () => void;
  } = $props();

  // 含 Risky 必须 type-to-confirm；纯非 Risky 批量走模态但不强制 token。
  const requiresToken = $derived(items.some((i) => i.safety === "Risky"));
  const totalSize = $derived(items.reduce((s, i) => s + i.size, 0));

  let input = $state("");
  const canDelete = $derived(!requiresToken || isConfirmed(input));

  function handleDelete() {
    // 不绑定 Enter：必须点已启用按钮（DESIGN.md §6 / U8）。
    if (canDelete) onConfirm(input);
  }
</script>

<div
  class="backdrop"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onCancel();
  }}
>
  <div class="modal" role="dialog" aria-modal="true" aria-labelledby="confirm-title">
    <header>
      <h2 id="confirm-title">确认删除 {items.length} 项</h2>
      <span class="total">{formatBytes(totalSize)}</span>
    </header>

    <ul class="items">
      {#each items as item (item.path)}
        <li>
          {#if item.safety}
            <Safety safety={item.safety} />
          {/if}
          <PathText path={item.path} />
          <EvidenceCard impact={item.impact ?? ""} recovery={item.recovery ?? ""} />
          <span class="size">{formatBytes(item.size)}</span>
        </li>
      {/each}
    </ul>

    {#if requiresToken}
      <div class="token-gate">
        <p class="warn">
          含<strong>危险</strong>项。请输入 <code>{CONFIRM_TOKEN}</code> 以确认删除。
        </p>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          type="text"
          bind:value={input}
          placeholder={CONFIRM_TOKEN}
          spellcheck="false"
          autocapitalize="off"
          autocomplete="off"
          aria-label="输入 {CONFIRM_TOKEN} 以确认"
          autofocus
        />
      </div>
    {/if}

    <footer>
      <p class="trash-note">移入废纸篓，可恢复。</p>
      <div class="actions">
        <button class="cancel" onclick={onCancel}>取消</button>
        <button class="delete" disabled={!canDelete} onclick={handleDelete}>
          删除
        </button>
      </div>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: color-mix(in oklch, var(--surface-base) 70%, transparent);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
    padding: var(--sp-6);
  }
  .modal {
    background: var(--surface-overlay);
    border: 1px solid var(--state-danger);
    border-radius: var(--radius);
    width: min(640px, 100%);
    max-height: min(80vh, 100%);
    display: flex;
    flex-direction: column;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.5);
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--sp-4);
    padding: var(--sp-4);
    border-bottom: 1px solid var(--border-subtle);
  }
  h2 {
    margin: 0;
    font-size: 1rem;
    color: var(--state-warning);
  }
  .total {
    font-family: var(--font-mono);
    color: var(--state-success);
  }
  .items {
    list-style: none;
    margin: 0;
    padding: var(--sp-2) var(--sp-4);
    overflow-y: auto;
    flex: 1 1 auto;
  }
  .items li {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: var(--row-height);
    border-bottom: 1px solid color-mix(in oklch, var(--border-subtle) 40%, transparent);
  }
  .size {
    font-family: var(--font-mono);
    color: var(--ink-muted);
    flex: 0 0 auto;
  }
  .token-gate {
    padding: var(--sp-3) var(--sp-4);
    border-top: 1px solid var(--border-subtle);
  }
  .warn {
    margin: 0 0 var(--sp-2);
    color: var(--state-warning);
    font-size: 0.9em;
  }
  .warn code {
    font-family: var(--font-mono);
    background: var(--surface-raised);
    padding: 0 var(--sp-1);
    border-radius: 3px;
  }
  .token-gate input {
    width: 100%;
    padding: var(--sp-2) var(--sp-3);
    background: var(--surface-base);
    border: 1px solid var(--state-warning);
    border-radius: var(--radius);
    color: var(--ink-primary);
    font-family: var(--font-mono);
    font-size: 1em;
  }
  .token-gate input:focus {
    outline: 2px solid var(--state-warning);
    outline-offset: 1px;
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
    padding: var(--sp-4);
    border-top: 1px solid var(--border-subtle);
  }
  .trash-note {
    margin: 0;
    color: var(--ink-muted);
    font-size: 0.85em;
  }
  .actions {
    display: flex;
    gap: var(--sp-2);
  }
  button {
    font-family: var(--font-ui);
    font-size: 0.9em;
    padding: var(--sp-2) var(--sp-4);
    border-radius: var(--radius);
    border: 1px solid var(--border-subtle);
    background: var(--surface-raised);
    color: var(--ink-primary);
    cursor: pointer;
  }
  .cancel:hover {
    background: var(--surface-overlay);
    border-color: var(--ink-muted);
  }
  .delete {
    border-color: var(--state-danger);
    color: var(--state-danger);
    font-weight: 600;
  }
  .delete:hover:not(:disabled) {
    background: color-mix(in oklch, var(--state-danger) 18%, var(--surface-raised));
  }
  .delete:disabled {
    color: var(--ink-faint);
    border-color: var(--border-subtle);
    cursor: not-allowed;
  }
</style>
