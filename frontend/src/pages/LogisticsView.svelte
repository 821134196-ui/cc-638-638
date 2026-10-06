<script>
  import { onMount } from 'svelte';
  import { api } from '../api.js';
  import { toast } from '../toast.js';
  import ScanBox from '../components/ScanBox.svelte';

  export let classes;

  let tab = 'suggestions';
  let models = [];

  // 建议 / 分配
  let suggestions = [];
  let generating = false;
  let filterClass = '';
  let assignTarget = null; // 当前要分配的建议行
  let candidates = [];
  let candidateLoading = false;
  let pickedCode = '';
  let assignBusy = false;

  // 资产
  let assets = [];
  let assetClass = '';
  let assetStatus = '';
  let assetModel = '';
  let onlyAvailable = false;
  let busyCode = null;

  // 调拨
  let transfers = [];
  let transferStatus = 'pending';
  let newTransfer = { asset_code: '', to_class_id: '' };
  let transferBusy = false;

  // 换班
  let allStudents = [];
  let moveDrafts = {}; // studentId -> class id

  async function loadModels() {
    models = await api.get('/models');
  }

  async function loadSuggestions() {
    const q = filterClass ? `&class_id=${filterClass}` : '';
    suggestions = await api.get(`/suggestions?status=open${q}`);
  }

  async function generate() {
    generating = true;
    try {
      const r = await api.post('/suggestions/generate');
      toast.success(`已重算：新增 ${r.created} 条，自动解除 ${r.obsoleted} 条；未改动任何资产`);
      await loadSuggestions();
    } catch (e) {
      toast.error(e.message);
    } finally {
      generating = false;
    }
  }

  async function openAssign(s) {
    assignTarget = s;
    pickedCode = '';
    candidates = [];
    candidateLoading = true;
    try {
      const q = filterClass ? `class_id=${filterClass}&` : '';
      candidates = await api.get(`/assets?${q}available=1&model=${s.model}`);
      if (candidates.length === 0) toast.info(`全校暂无 ${s.model} 号空余完好桌椅，可考虑跨班调拨`);
    } finally {
      candidateLoading = false;
    }
  }

  async function doAssign() {
    const code = (pickedCode || '').trim().toUpperCase();
    if (!code) return toast.error('请选择或输入桌椅编号');
    assignBusy = true;
    try {
      await api.post('/assignments', {
        student_id: assignTarget.student_id,
        asset_code: code,
        reason: `按身高匹配建议（${assignTarget.model} 号）`,
      });
      toast.success(`已把 ${code} 分配给 ${assignTarget.student_name}`);
      assignTarget = null;
      await Promise.all([loadSuggestions(), loadAssets()]);
    } catch (e) {
      toast.error(e.message);
    } finally {
      assignBusy = false;
    }
  }

  async function loadAssets() {
    const params = new URLSearchParams();
    if (assetClass) params.set('class_id', assetClass);
    if (assetStatus) params.set('status', assetStatus);
    if (assetModel) params.set('model', assetModel);
    if (onlyAvailable) params.set('available', '1');
    assets = await api.get(`/assets?${params.toString()}`);
  }

  async function release(a) {
    if (!confirm(`确定收回 ${a.code}（${a.occupant_name ?? ''}在用）吗？历史记录会保留。`)) return;
    busyCode = a.code;
    try {
      await api.post(`/assets/${a.code}/release`);
      toast.success('已收回');
      await loadAssets();
    } catch (e) { toast.error(e.message); } finally { busyCode = null; }
  }

  async function markDamaged(a) {
    const note = prompt(`登记 ${a.code} 的损坏情况：`, '');
    if (note === null) return;
    busyCode = a.code;
    try {
      await api.post(`/assets/${a.code}/damaged`, { note });
      toast.success('已标记损坏，停止占用且退出可分配列表');
      await loadAssets();
    } catch (e) { toast.error(e.message); } finally { busyCode = null; }
  }

  async function repair(a) {
    busyCode = a.code;
    try {
      await api.post(`/assets/${a.code}/repair`);
      toast.success('已修复，恢复可分配');
      await loadAssets();
    } catch (e) { toast.error(e.message); } finally { busyCode = null; }
  }

  async function loadTransfers() {
    transfers = await api.get(`/transfers?status=${transferStatus}`);
  }

  async function createTransfer() {
    const code = newTransfer.asset_code.trim().toUpperCase();
    if (!code || !newTransfer.to_class_id) return toast.error('请填写桌椅编号与接收班级');
    transferBusy = true;
    try {
      const r = await api.post('/transfers', { asset_code: code, to_class_id: Number(newTransfer.to_class_id) });
      toast.success(`调拨单已创建：班级#${r.from_class_id} → 班级#${r.to_class_id}，等待双方班主任确认`);
      newTransfer = { asset_code: '', to_class_id: '' };
      await loadTransfers();
      await loadAssets();
    } catch (e) { toast.error(e.message); } finally { transferBusy = false; }
  }

  async function cancelTransfer(t) {
    if (!confirm(`取消调拨单 #${t.id}（${t.asset_code}）？`)) return;
    try {
      await api.post(`/transfers/${t.id}/cancel`, { reason: '后勤取消' });
      toast.success('已取消');
      await loadTransfers();
    } catch (e) { toast.error(e.message); }
  }

  async function loadStudents() {
    const lists = await Promise.all(classes.map((c) => api.get(`/classes/${c.id}/students`)));
    allStudents = lists.flat().map((s) => ({ ...s }));
    const d = {};
    for (const s of allStudents) d[s.id] = s.class_id;
    moveDrafts = d;
  }

  async function moveStudent(s) {
    const target = Number(moveDrafts[s.id]);
    if (!target || target === s.class_id) return;
    if (!confirm(`将 ${s.name} 调到 ${className(target)}？其原桌椅保留在 ${className(s.class_id)}，历史记录保留。`)) {
      moveDrafts[s.id] = s.class_id;
      return;
    }
    try {
      await api.patch(`/students/${s.id}/class`, { class_id: target });
      toast.success('换班成功：资产未移动，请为该生在新班重新调配');
      s.class_id = target;
      await Promise.all([loadSuggestions(), loadAssets()]);
    } catch (e) { toast.error(e.message); moveDrafts[s.id] = s.class_id; }
  }

  function className(id) {
    return classes.find((c) => c.id === id)?.name || `班级#${id}`;
  }
  function modelName(code) {
    return models.find((m) => m.code === code)?.name || `${code} 号`;
  }

  onMount(async () => {
    await loadModels();
    await Promise.all([loadSuggestions(), loadAssets(), loadTransfers(), loadStudents()]);
  });
</script>

<!-- ============ 待办建议与分配 ============ -->
{#if tab === 'suggestions'}
  <div class="card">
    <div class="row">
      <h2 style="margin:0">🔍 身高匹配建议（后勤核实后手动分配）</h2>
      <span class="spacer"></span>
      <select bind:value={filterClass} on:change={loadSuggestions}>
        <option value="">全部班级</option>
        {#each classes as c}<option value={c.id}>{c.name}</option>{/each}
      </select>
      <button class="primary" on:click={generate} disabled={generating}>
        {generating ? '重算中…' : '按最新身高重新生成建议'}
      </button>
    </div>
    <p class="muted" style="margin-bottom:0">
      建议只列出不匹配学生与所需型号，<b>不会</b>占用或移动任何资产；点击“安排调换”从现有空余资产中选择。
    </p>
  </div>

  <div class="card">
    {#if suggestions.length === 0}
      <div class="empty">没有待处理的匹配建议 🎉</div>
    {:else}
      <table>
        <thead>
          <tr><th>学生</th><th>班级</th><th>身高</th><th>需配</th><th>当前在用</th><th>原因</th><th></th></tr>
        </thead>
        <tbody>
          {#each suggestions as s (s.id)}
            <tr>
              <td>{s.student_name}</td>
              <td>{className(s.class_id)}</td>
              <td>{s.height_cm} cm</td>
              <td><span class="badge info">{modelName(s.model)}</span></td>
              <td>
                {#if s.current_asset}
                  <span class="mono">{s.current_asset}</span>（{s.current_model} 号）
                {:else}<span class="muted">无桌椅</span>{/if}
              </td>
              <td class="muted">{s.reason}</td>
              <td><button class="primary tiny" on:click={() => openAssign(s)}>安排调换</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </div>

{:else if tab === 'assets'}
  <!-- ============ 资产台账 ============ -->
  <div class="card">
    <div class="row filters">
      <h2 style="margin:0">🏗️ 资产台账</h2>
      <span class="spacer"></span>
      <select bind:value={assetClass} on:change={loadAssets}>
        <option value="">全部班级</option>
        {#each classes as c}<option value={c.id}>{c.name}</option>{/each}
      </select>
      <select bind:value={assetModel} on:change={loadAssets}>
        <option value="">全部型号</option>
        {#each models as m}<option value={m.code}>{m.name}（{m.height_min}~{m.height_max}cm）</option>{/each}
      </select>
      <select bind:value={assetStatus} on:change={loadAssets}>
        <option value="">全部状态</option>
        <option value="ok">完好</option>
        <option value="damaged">损坏</option>
      </select>
      <label class="row chk">
        <input type="checkbox" bind:checked={onlyAvailable} on:change={loadAssets} /> 仅可分配
      </label>
    </div>
    <p class="muted" style="margin-bottom:0">
      “可分配”= 完好 + 未被学生占用 + 无进行中调拨。损坏品永远不会出现在分配候选中。
    </p>
  </div>

  <div class="card">
    <table>
      <thead>
        <tr><th>编号</th><th>型号</th><th>所在班</th><th>状态</th><th>在用学生</th><th>调拨</th><th>操作</th></tr>
      </thead>
      <tbody>
        {#each assets as a (a.code)}
          <tr>
            <td class="mono">{a.code}</td>
            <td>{a.model} 号</td>
            <td>{className(a.class_id)}</td>
            <td>
              {#if a.status === 'damaged'}<span class="badge danger">损坏</span>
              {:else}<span class="badge ok">完好</span>{/if}
              {#if a.note}<span class="muted"> {a.note}</span>{/if}
            </td>
            <td>{a.occupant_name ?? '—'}</td>
            <td>
              {#if a.pending_transfer_to_class}
                <span class="badge warn">调拨中 → {className(a.pending_transfer_to_class)}</span>
              {:else}—{/if}
            </td>
            <td class="row">
              {#if a.status === 'ok' && a.occupant_student_id}
                <button class="tiny" disabled={busyCode === a.code} on:click={() => release(a)}>收回</button>
              {/if}
              {#if a.status === 'ok'}
                <button class="danger tiny" disabled={busyCode === a.code} on:click={() => markDamaged(a)}>报损</button>
              {:else}
                <button class="tiny" disabled={busyCode === a.code} on:click={() => repair(a)}>修复</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

{:else if tab === 'transfers'}
  <!-- ============ 跨班调拨 ============ -->
  <div class="card">
    <h2>🔁 发起跨班调拨</h2>
    <div class="row">
      <input placeholder="空余桌椅编号，如 DESK-0020" bind:value={newTransfer.asset_code} style="width:220px" />
      <select bind:value={newTransfer.to_class_id}>
        <option value="">接收班级…</option>
        {#each classes as c}<option value={c.id}>{c.name}</option>{/each}
      </select>
      <button class="primary" on:click={createTransfer} disabled={transferBusy}>
        {transferBusy ? '提交中…' : '发起调拨（交出班级为资产当前所在班）'}
      </button>
    </div>
    <p class="muted" style="margin-bottom:0">
      发起后资产仍留在原班；必须<b>交出方与接收方班主任双方确认</b>，资产位置才会更新。
    </p>
  </div>

  <div class="card">
    <div class="row">
      <h2 style="margin:0">调拨单</h2>
      <span class="spacer"></span>
      <select bind:value={transferStatus} on:change={loadTransfers}>
        <option value="pending">待确认</option>
        <option value="completed">已完成</option>
        <option value="cancelled">已取消</option>
      </select>
    </div>
    <table style="margin-top:10px">
      <thead>
        <tr><th>#</th><th>桌椅</th><th>交出方</th><th>接收方</th><th>状态</th><th></th></tr>
      </thead>
      <tbody>
        {#each transfers as t (t.id)}
          <tr>
            <td>{t.id}</td>
            <td class="mono">{t.asset_code}</td>
            <td>{className(t.from_class_id)} {#if t.from_confirmed_by}<span class="badge ok">已确认</span>{:else}<span class="badge muted">待确认</span>{/if}</td>
            <td>{className(t.to_class_id)} {#if t.to_confirmed_by}<span class="badge ok">已确认</span>{:else}<span class="badge muted">待确认</span>{/if}</td>
            <td>
              {#if t.status === 'pending'}<span class="badge warn">待双方确认</span>
              {:else if t.status === 'completed'}<span class="badge ok">已移交</span>
              {:else}<span class="badge muted">已取消</span>{/if}
            </td>
            <td>{#if t.status === 'pending'}<button class="danger tiny" on:click={() => cancelTransfer(t)}>取消</button>{/if}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

{:else if tab === 'moves'}
  <!-- ============ 学生换班 ============ -->
  <div class="card">
    <h2>🏫 学生换班</h2>
    <p class="muted" style="margin-top:-4px">
      换班只改变学生所属班级；学生原用桌椅<b>保留在原班</b>并结束占用，原分配与调拨记录全部保留。
    </p>
    <table>
      <thead><tr><th>学号</th><th>姓名</th><th>现班级</th><th>调整到</th><th></th></tr></thead>
      <tbody>
        {#each allStudents as s (s.id)}
          <tr>
            <td>{s.id}</td>
            <td>{s.name}</td>
            <td>{className(s.class_id)}</td>
            <td>
              <select bind:value={moveDrafts[s.id]}>
                {#each classes as c}<option value={c.id}>{c.name}</option>{/each}
              </select>
            </td>
            <td>
              <button class="primary tiny" disabled={Number(moveDrafts[s.id]) === s.class_id}
                      on:click={() => moveStudent(s)}>办理换班</button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

{:else if tab === 'scan'}
  <ScanBox on:scanned={() => loadAssets()} />
{/if}

<!-- 分配弹窗 -->
{#if assignTarget}
  <div class="modal-mask" on:click={() => (assignTarget = null)} on:keydown role="presentation">
    <div class="modal" on:click|stopPropagation role="dialog" aria-modal="true">
      <h3>为 {assignTarget.student_name}（{className(assignTarget.class_id)}）安排 {modelName(assignTarget.model)}</h3>
      <p class="muted">身高 {assignTarget.height_cm}cm；{assignTarget.reason}</p>
      {#if candidateLoading}
        <div class="empty">正在查找可用资产…</div>
      {:else if candidates.length === 0}
        <div class="empty">
          全校没有 {assignTarget.model} 号的空余完好桌椅。<br />
          可先在“跨班调拨”中等待他班调出，或在下方手工输入编号核实。
        </div>
      {:else}
        <div class="cand-list">
          {#each candidates as c (c.code)}
              <label class="cand" class:sel={pickedCode === c.code}>
                <input type="radio" name="cand" bind:group={pickedCode} value={c.code} />
                <span class="mono">{c.code}</span>
                <span>{c.model} 号</span>
                <span class="muted">{className(c.class_id)}</span>
              </label>
          {/each}
        </div>
      {/if}
      <div class="row" style="margin-top:12px">
        <input placeholder="也可手工输入编号核实" bind:value={pickedCode} style="width:200px" />
      </div>
      <div class="row" style="margin-top:14px">
        <span class="spacer"></span>
        <button on:click={() => (assignTarget = null)}>取消</button>
        <button class="primary" on:click={doAssign} disabled={assignBusy || !pickedCode}>
          {assignBusy ? '分配中…' : '确认分配'}
        </button>
      </div>
      <p class="muted warn-note">
        多人同时领用同一套桌椅时，系统只允许一个分配成功，其余请求会收到冲突提示。
      </p>
    </div>
  </div>
{/if}

<!-- 后勤页签，tab=scan 时也提供统一入口 -->
<div class="dock">
  <button class:active={tab === 'suggestions'} on:click={() => (tab = 'suggestions')}>待办建议</button>
  <button class:active={tab === 'assets'} on:click={() => (tab = 'assets')}>资产台账</button>
  <button class:active={tab === 'transfers'} on:click={() => (tab = 'transfers')}>跨班调拨</button>
  <button class:active={tab === 'moves'} on:click={() => (tab = 'moves')}>学生换班</button>
  <button class:active={tab === 'scan'} on:click={() => (tab = 'scan')}>扫码核验</button>
</div>

<style>
  .filters select { min-width: 110px; }
  .chk { font-size: 12px; color: var(--muted); gap: 5px; }
  .dock {
    position: sticky; bottom: 14px;
    background: rgba(255,255,255,.92); backdrop-filter: blur(6px);
    border: 1px solid var(--border); border-radius: 999px;
    padding: 7px; display: flex; gap: 6px; width: fit-content;
    margin: 6px auto 0; box-shadow: 0 8px 24px rgba(15,23,42,.10);
  }
  .dock button.active { background: var(--brand); border-color: var(--brand); color: #fff; }

  .modal-mask {
    position: fixed; inset: 0; background: rgba(15,23,42,.45);
    display: flex; align-items: center; justify-content: center; z-index: 40; padding: 20px;
  }
  .modal {
    background: #fff; border-radius: 14px; padding: 22px; width: 560px; max-width: 100%;
    max-height: 85vh; overflow-y: auto; box-shadow: 0 24px 60px rgba(0,0,0,.25);
  }
  .modal h3 { margin: 0 0 4px; }
  .cand-list { display: flex; flex-direction: column; gap: 8px; margin-top: 10px; }
  .cand {
    display: flex; gap: 12px; align-items: center; padding: 9px 12px;
    border: 1px solid var(--border); border-radius: 10px; cursor: pointer;
  }
  .cand.sel { border-color: var(--brand); background: var(--info-bg); }
  .warn-note { font-size: 12px; margin-bottom: 0; color: var(--warn); }
</style>
