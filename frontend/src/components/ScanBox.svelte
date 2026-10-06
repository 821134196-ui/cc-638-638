<script>
  import { createEventDispatcher } from 'svelte';
  import { api } from '../api.js';
  import { toast } from '../toast.js';

  const dispatch = createEventDispatcher();
  let code = '';
  let result = null;
  let loading = false;

  async function scan() {
    const c = code.trim().toUpperCase();
    if (!c) return;
    loading = true;
    result = null;
    try {
      result = await api.post('/assets/scan', { code: c });
      dispatch('scanned', result);
      if (result.found) {
        result.allocatable
          ? toast.success(`${c} 可分配（${result.asset.model} 号桌）`)
          : toast.error(`${c} 暂不可分配：${result.blockers.join('、')}`);
      } else {
        toast.error(`未找到编号 ${c}`);
      }
    } catch (e) {
      toast.error(e.message);
    } finally {
      loading = false;
    }
  }
</script>

<div class="scan card">
  <h2>📷 扫码（本地模拟）</h2>
  <div class="row">
    <input bind:value={code} placeholder="输入或扫描桌椅编号，如 DESK-0001"
           class="code-input" on:keydown={(e) => e.key === 'Enter' && scan()} />
    <button class="primary" on:click={scan} disabled={loading}>{loading ? '查询中…' : '扫描'}</button>
  </div>
  {#if result && result.found}
    <div class="result {result.allocatable ? 'good' : 'bad'}">
      <span class="mono">{result.asset.code}</span>
      <span>{result.asset.model} 号桌</span>
      <span>班级 #{result.asset.class_id}</span>
      {#if result.asset.occupant_name}
        <span>在用：{result.asset.occupant_name}</span>
      {/if}
      <span class="badge {result.allocatable ? 'ok' : 'danger'}">
        {result.allocatable ? '可分配' : result.blockers.join(' / ')}
      </span>
      <span class="muted device">设备：{result.device}</span>
    </div>
  {/if}
</div>

<style>
  .code-input { width: 300px; max-width: 100%; }
  .result {
    margin-top: 12px; padding: 10px 12px; border-radius: 8px;
    display: flex; gap: 12px; align-items: center; flex-wrap: wrap;
    border: 1px solid;
  }
  .result.good { background: var(--ok-bg); border-color: #bbf7d0; }
  .result.bad { background: var(--danger-bg); border-color: #fecaca; }
  .device { margin-left: auto; font-size: 12px; }
</style>
