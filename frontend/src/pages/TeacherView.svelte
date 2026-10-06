<script>
  import { onMount } from 'svelte';
  import { api } from '../api.js';
  import { toast } from '../toast.js';
  import ScanBox from '../components/ScanBox.svelte';

  export let classId;
  export let className;

  let tab = 'pending';
  let board = null;
  let students = [];
  let transfers = [];
  let heights = {}; // studentId -> 输入框草稿
  let savingId = null;
  let busyTransfer = null;

  async function loadAll() {
    await Promise.all([loadBoard(), loadStudents(), loadTransfers()]);
  }

  async function loadBoard() {
    board = await api.get(`/classes/${classId}/board`);
  }
  async function loadStudents() {
    students = await api.get(`/classes/${classId}/students`);
    const h = {};
    for (const s of students) h[s.id] = s.height_cm ?? '';
    heights = h;
  }
  async function loadTransfers() {
    transfers = await api.get('/transfers?status=pending');
  }

  async function saveHeight(s) {
    const v = Number(heights[s.id]);
    if (!v || v < 100 || v > 180) {
      toast.error('身高需在 100~180 cm 之间');
      heights[s.id] = s.height_cm ?? '';
      return;
    }
    savingId = s.id;
    try {
      const r = await api.patch(`/students/${s.id}/height`, { height_cm: v });
      s.height_cm = v;
      s.desired_model = r.desired_model;
      toast.success(
        r.suggestion_created
          ? `已更新，建议 ${r.desired_model} 号桌，已生成待调换建议`
          : `已更新，当前桌椅型号匹配`
      );
      await loadBoard();
    } catch (e) {
      toast.error(e.message);
    } finally {
      savingId = null;
    }
  }

  function sideOf(t) {
    if (t.from_class_id === classId) return 'from';
    if (t.to_class_id === classId) return 'to';
    return null;
  }

  async function confirmTransfer(t) {
    busyTransfer = t.id;
    try {
      const r = await api.post(`/transfers/${t.id}/confirm`);
      toast.success(r.completed ? '双方确认完成，资产已移交本班/对方班级' : '已确认，等待另一方确认');
      await Promise.all([loadBoard(), loadTransfers()]);
    } catch (e) {
      toast.error(e.message);
    } finally {
      busyTransfer = null;
    }
  }

  async function cancelTransfer(t) {
    if (!confirm('确定取消这笔调拨吗？')) return;
    try {
      await api.post(`/transfers/${t.id}/cancel`, { reason: '班主任取消' });
      toast.success('调拨已取消');
      await Promise.all([loadBoard(), loadTransfers()]);
    } catch (e) {
      toast.error(e.message);
    }
  }

  onMount(loadAll);

  function currentDesk(studentId) {
    return board?.assigned?.find((a) => a.student_id === studentId) || null;
  }

  $: myTransfers = transfers.filter((t) => sideOf(t) !== null);
</script>

<div class="stat-row">
  <div class="stat warn"><div class="num">{board?.pending_swaps?.length ?? 0}</div><div class="label">待调换学生</div></div>
  <div class="stat ok"><div class="num">{board?.assigned?.length ?? 0}</div><div class="label">已分配在用</div></div>
  <div class="stat"><div class="num">{board?.available?.length ?? 0}</div><div class="label">本班空余完好</div></div>
  <div class="stat danger"><div class="num">{board?.damaged?.length ?? 0}</div><div class="label">损坏桌椅</div></div>
</div>

<div class="card">
  <h2>👨‍👩‍👧 学生身高登记 · {className}</h2>
  <p class="muted" style="margin-top:-4px">
    更新身高后，系统会按身高区间生成“建议”。建议仅供后勤核实调配，<b>不会</b>自动改动任何桌椅归属。
  </p>
  <div class="table-wrap">
    <table>
      <thead>
        <tr><th>学号</th><th>姓名</th><th>身高 cm</th><th>应配型号</th><th>当前在用</th><th></th></tr>
      </thead>
      <tbody>
        {#each students as s (s.id)}
          <tr>
            <td>{s.id}</td>
            <td>{s.name}</td>
            <td>
              <input type="number" min="100" max="180" bind:value={heights[s.id]}
                     style="width:90px" />
            </td>
            <td>
              {#if s.desired_model}<span class="badge info">{s.desired_model} 号</span>{:else}<span class="muted">—</span>{/if}
            </td>
            <td>
              <!-- 从看板已分配里找当前桌椅 -->
              {#if currentDesk(s.id)}
                {@const cur = currentDesk(s.id)}
                <span class="mono">{cur.asset_code}</span>
                <span class="badge {cur.mismatch ? 'warn' : 'ok'}">
                  {cur.model} 号{cur.mismatch ? ' · 不符' : ' · 匹配'}
                </span>
              {:else}
                <span class="muted">未分配</span>
              {/if}
            </td>
            <td>
              <button class="primary tiny" disabled={savingId === s.id || heights[s.id] === '' || Number(heights[s.id]) === s.height_cm}
                      on:click={() => saveHeight(s)}>
                {savingId === s.id ? '保存中…' : '更新身高'}
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</div>

<div class="card">
  <h2>📋 班级桌椅看板</h2>
  <div class="tabs">
    <button class:active={tab === 'pending'} on:click={() => (tab = 'pending')}>
      待调换 ({board?.pending_swaps?.length ?? 0})
    </button>
    <button class:active={tab === 'assigned'} on:click={() => (tab = 'assigned')}>
      已分配 ({board?.assigned?.length ?? 0})
    </button>
    <button class:active={tab === 'available'} on:click={() => (tab = 'available')}>
      空余完好 ({board?.available?.length ?? 0})
    </button>
    <button class:active={tab === 'damaged'} on:click={() => (tab = 'damaged')}>
      损坏 ({board?.damaged?.length ?? 0})
    </button>
    <button class:active={tab === 'transfers'} on:click={() => (tab = 'transfers')}>
      跨班调拨 ({myTransfers.length})
    </button>
  </div>

  {#if !board}
    <div class="empty">加载中…</div>
  {:else if tab === 'pending'}
    {#if board.pending_swaps.length === 0}
      <div class="empty">暂无待调换学生 🎉</div>
    {:else}
      <table>
        <thead><tr><th>学生</th><th>身高</th><th>建议型号</th><th>当前在用</th><th>原因</th></tr></thead>
        <tbody>
          {#each board.pending_swaps as p (p.id)}
            <tr>
              <td>{p.student_name}</td>
              <td>{p.height_cm} cm</td>
              <td><span class="badge info">{p.model} 号</span></td>
              <td>
                {#if p.current_asset}
                  <span class="mono">{p.current_asset}</span>（{p.current_model} 号）
                {:else}<span class="muted">无</span>{/if}
              </td>
              <td class="muted">{p.reason}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

  {:else if tab === 'assigned'}
    <table>
      <thead><tr><th>桌椅编号</th><th>型号</th><th>使用学生</th><th>身高/应配</th><th>状态</th></tr></thead>
      <tbody>
        {#each board.assigned as a (a.asset_code)}
          <tr>
            <td class="mono">{a.asset_code}</td>
            <td>{a.model} 号</td>
            <td>{a.student_name}</td>
            <td>{a.height_cm ?? '—'} cm / {a.desired_model ?? '—'} 号</td>
            <td>
              <span class="badge {a.mismatch ? 'warn' : 'ok'}">{a.mismatch ? '型号不符' : '匹配'}</span>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>

  {:else if tab === 'available'}
    {#if board.available.length === 0}
      <div class="empty">本班暂无空余完好桌椅，可请后勤从其他班调拨</div>
    {:else}
      <table>
        <thead><tr><th>桌椅编号</th><th>型号</th><th>状态</th></tr></thead>
        <tbody>
          {#each board.available as a (a.code)}
            <tr>
              <td class="mono">{a.code}</td>
              <td>{a.model} 号</td>
              <td><span class="badge ok">完好·可分配</span></td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

  {:else if tab === 'damaged'}
    {#if board.damaged.length === 0}
      <div class="empty">没有损坏登记</div>
    {:else}
      <table>
        <thead><tr><th>桌椅编号</th><th>型号</th><th>备注</th></tr></thead>
        <tbody>
          {#each board.damaged as d (d.code)}
            <tr>
              <td class="mono">{d.code}</td>
              <td>{d.model} 号</td>
              <td><span class="badge danger">损坏</span> {d.note}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

  {:else if tab === 'transfers'}
    {#if myTransfers.length === 0}
      <div class="empty">暂无与本班相关的跨班调拨</div>
    {:else}
      <table>
        <thead>
          <tr><th>调拨单</th><th>桌椅</th><th>交出方</th><th>接收方</th><th>确认进度</th><th>操作</th></tr>
        </thead>
        <tbody>
          {#each myTransfers as t (t.id)}
            {@const side = sideOf(t)}
            {@const myDone = side === 'from' ? !!t.from_confirmed_by : !!t.to_confirmed_by}
            <tr>
              <td>#{t.id}</td>
              <td class="mono">{t.asset_code}</td>
              <td>班级#{t.from_class_id} {#if t.from_confirmed_by}<span class="badge ok">已确认</span>{:else}<span class="badge muted">待确认</span>{/if}</td>
              <td>班级#{t.to_class_id} {#if t.to_confirmed_by}<span class="badge ok">已确认</span>{:else}<span class="badge muted">待确认</span>{/if}</td>
              <td>
                {#if side === 'from'}我方为交出方{:else}我方为接收方{/if}
              </td>
              <td class="row">
                {#if myDone}
                  <span class="badge ok">我方已确认</span>
                {:else}
                  <button class="primary tiny" disabled={busyTransfer === t.id} on:click={() => confirmTransfer(t)}>
                    {busyTransfer === t.id ? '处理中…' : '确认调拨'}
                  </button>
                {/if}
                <button class="danger tiny" disabled={busyTransfer === t.id} on:click={() => cancelTransfer(t)}>取消</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="muted" style="margin-bottom:0;font-size:12px">
        规则：必须交出方与接收方班主任都确认后，资产位置才会更新；仅一方确认时资产不动。
      </p>
    {/if}
  {/if}
</div>

<ScanBox on:scanned={() => loadBoard()} />

<style>
  .table-wrap { overflow-x: auto; }
</style>
