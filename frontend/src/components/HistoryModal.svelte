<script>
  import { createEventDispatcher, onMount } from 'svelte';
  import { api } from '../lib/api.js';

  export let assetCode;
  const dispatch = createEventDispatcher();
  let events = [];
  let loading = true;
  let error = '';

  const EVENT_LABEL = {
    assign: '分配',
    unassign: '收回',
    transfer_requested: '发起调拨',
    transfer_released: '交出方放行',
    transfer_confirmed: '接收方确认·资产移动',
    transfer_rejected: '调拨拒绝',
    damaged: '报修损坏',
    repaired: '维修入库',
    height_update: '身高更新（未动资产）',
    student_transfer: '学生换班（资产留原班）',
  };

  onMount(async () => {
    try {
      const r = await api.get(`/assets/${assetCode}/history`);
      events = r.history;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  });
</script>

<div style="position:fixed;inset:0;background:rgba(20,30,45,.35);display:grid;place-items:center;z-index:50"
     on:click={() => dispatch('close')} on:keydown={() => {}}>
  <div class="card" style="width:640px;max-height:80vh;overflow:auto"
       on:click|stopPropagation on:keydown|stopPropagation>
       role="dialog" aria-label="资产履历">
    <div class="section-title">
      <h2 style="font-size:16px">资产履历 · <code>{assetCode}</code></h2>
      <button on:click={() => dispatch('close')}>关闭</button>
    </div>
    <p class="muted small">所有分配、收回、调拨节点、损坏与维修均只追加记录，不修改不删除。</p>
    {#if loading}
      <p class="muted">加载中…</p>
    {:else if error}
      <p style="color:var(--red)">{error}</p>
    {:else if events.length === 0}
      <p class="muted">暂无记录</p>
    {:else}
      <table>
        <thead><tr><th>时间</th><th>事件</th><th>班级变化</th><th>学生变化</th><th>说明</th></tr></thead>
        <tbody>
          {#each events as e (e.id)}
            <tr>
              <td class="small">{e.created_at}</td>
              <td><span class="tag gray">{EVENT_LABEL[e.event_type] || e.event_type}</span></td>
              <td class="small">{e.from_class_id ?? '仓库'} → {e.to_class_id ?? '—'}</td>
              <td class="small">{e.from_student_id ?? '—'} → {e.to_student_id ?? '—'}</td>
              <td class="small muted">{e.detail || ''}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>
</div>
