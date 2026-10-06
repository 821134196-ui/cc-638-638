<script>
  import { onMount } from 'svelte';
  import { api, toast, user, STATUS_LABEL, KIND_LABEL } from '../lib/api.js';
  import HistoryModal from './HistoryModal.svelte';

  export let logout;

  let tab = 'suggestions';
  let overview = null;
  let suggestions = [];
  let assets = [];
  let transfers = [];
  let classes = [];
  let busy = {};

  // 新建调拨表单
  let newTransfer = { asset_code: '', from_class_id: '', to_class_id: '' };
  // 扫码模拟
  let scanCode = '';
  let scanResult = null;
  // 换班
  let moveForm = { student_id: '', class_id: '' };
  let historyAsset = null;

  async function refresh() {
    try {
      if (tab === 'suggestions' || tab === 'overview') {
        overview = await api.get('/logistics/overview');
        const s = await api.get('/logistics/suggestions');
        suggestions = s.suggestions;
      }
      if (tab === 'assets' || tab === 'scan') {
        const a = await api.get('/assets');
        assets = a.assets;
      }
      if (tab === 'transfers') {
        const t = await api.get('/transfers');
        transfers = t.transfers;
        const c = await api.get('/classes');
        classes = c.classes;
      }
    } catch (e) {
      toast(e.message, 'err');
    }
  }

  onMount(refresh);

  async function allocate(studentId, assetCode, k) {
    busy[k] = true;
    try {
      const r = await api.post('/allocations', { student_id: studentId, asset_code: assetCode });
      toast(r.warning ? `已分配：${r.warning}` : `分配成功：${assetCode}`, r.warning ? 'info' : 'ok', 6000);
      await refresh();
    } catch (e) {
      toast(e.message, 'err');
    } finally {
      busy[k] = false;
    }
  }

  async function createTransferFromSuggestion(s, a, k) {
    busy[k] = true;
    try {
      const r = await api.post('/transfers', {
        asset_code: a.asset_code,
        from_class_id: a.location_class_id,
        to_class_id: s.class_id,
      });
      toast(`调拨单 #${r.transfer_id} 已创建：等待交出方放行 → 接收方确认，期间资产仍在原班`, 'info', 6000);
    } catch (e) {
      toast(e.message, 'err');
    } finally {
      busy[k] = false;
    }
  }

  async function createTransferManual() {
    if (!newTransfer.asset_code || !newTransfer.from_class_id || !newTransfer.to_class_id) {
      return toast('请填写桌椅编号、交出方与接收方', 'err');
    }
    if (Number(newTransfer.from_class_id) === Number(newTransfer.to_class_id)) {
      return toast('交出方与接收方不能相同', 'err');
    }
    try {
      const r = await api.post('/transfers', {
        asset_code: newTransfer.asset_code.trim().toUpperCase(),
        from_class_id: Number(newTransfer.from_class_id),
        to_class_id: Number(newTransfer.to_class_id),
      });
      toast(`调拨单 #${r.transfer_id} 已创建`, 'ok');
      newTransfer = { asset_code: '', from_class_id: '', to_class_id: '' };
      await refresh();
    } catch (e) {
      toast(e.message, 'err');
    }
  }

  async function transferAction(id, action) {
    try {
      const r = await api.post(`/transfers/${id}/${action}`, {});
      toast(r.notice || '操作成功', 'ok', 5000);
      await refresh();
    } catch (e) {
      toast(e.message, 'err');
    }
  }

  async function doScan() {
    if (!scanCode.trim()) return;
    try {
      scanResult = await api.get(`/assets/${scanCode.trim().toUpperCase()}`);
    } catch (e) {
      scanResult = null;
      toast(e.message, 'err');
    }
  }

  async function setDamage(code, damaged) {
    try {
      const r = damaged
        ? await api.post(`/assets/${code}/damage`, {})
        : await api.post(`/assets/${code}/repair`, {});
      toast(damaged ? '已报修，退出可分配列表' : '维修完成，恢复可分配', 'ok');
      await refresh();
      if (scanResult && scanResult.code === code) scanResult = await api.get(`/assets/${code}`);
    } catch (e) {
      toast(e.message, 'err');
    }
  }

  async function unassign(studentId) {
    try {
      const r = await api.post(`/students/${studentId}/unassign`, {});
      toast(`已收回 ${r.asset_code}，变为空闲可分配`, 'ok');
      await refresh();
    } catch (e) {
      toast(e.message, 'err');
    }
  }

  async function moveStudent() {
    if (!moveForm.student_id || !moveForm.class_id) return toast('请填写学生编号与目标班级', 'err');
    try {
      const r = await api.post(`/students/${moveForm.student_id}/transfer-class`, { class_id: Number(moveForm.class_id) });
      toast(r.notice, 'ok', 5000);
      moveForm = { student_id: '', class_id: '' };
      await refresh();
    } catch (e) {
      toast(e.message, 'err');
    }
  }

  function tag(s) { return STATUS_LABEL[s] || { text: s, cls: 'gray' }; }
</script>

<div style="max-width:1200px;margin:0 auto;padding:20px;">
  <div class="row" style="justify-content:space-between;margin-bottom:16px">
    <div>
      <h1 style="font-size:20px">后勤资产调拨台</h1>
      <span class="muted">{$user.display_name}</span>
    </div>
    <button on:click={logout}>退出登录</button>
  </div>

  <div class="tabs">
    <button class={tab === 'suggestions' ? 'active' : ''} on:click={() => { tab = 'suggestions'; refresh(); }}>不匹配与匹配建议</button>
    <button class={tab === 'assets' ? 'active' : ''} on:click={() => { tab = 'assets'; refresh(); }}>全部资产 / 扫码</button>
    <button class={tab === 'transfers' ? 'active' : ''} on:click={() => { tab = 'transfers'; refresh(); }}>跨班调拨</button>
    <button class={tab === 'overview' ? 'active' : ''} on:click={() => { tab = 'overview'; refresh(); }}>总览与换班</button>
  </div>

  {#if tab === 'suggestions'}
    <div class="grid cols-4" style="margin-bottom:16px">
      <div class="stat"><div class="num" style="color:var(--red)">{overview?.mismatch_count ?? '…'}</div><div class="label">不匹配学生</div></div>
      <div class="stat"><div class="num">{overview?.available ?? '…'}</div><div class="label">空闲可分配</div></div>
      <div class="stat"><div class="num" style="color:var(--red)">{overview?.damaged ?? '…'}</div><div class="label">损坏</div></div>
      <div class="stat"><div class="num" style="color:var(--purple)">{overview?.active_transfers ?? '…'}</div><div class="label">在途调拨</div></div>
    </div>

    <div class="card">
      <div class="section-title">
        <h2 style="font-size:16px">不匹配学生与可用资产建议</h2>
        <button on:click={refresh} class="small">重新计算建议</button>
      </div>
      <p class="muted small" style="margin-top:0">
        建议仅依据身高与现有空闲资产计算，<b>不会预留或改变任何资产归属</b>；请线下核实后点击“分配”。
      </p>
      {#if suggestions.length === 0}
        <p>全部学生桌椅均匹配 🎉</p>
      {:else}
        <div class="grid">
          {#each suggestions as s, i (s.student_id)}
            <div style="border:1px solid var(--border);border-radius:8px;padding:12px 14px;">
              <div class="row" style="justify-content:space-between">
                <div>
                  <b>{s.student_name}</b>
                  <span class="muted small">（{s.class_name} · {s.height_cm}cm）</span>
                </div>
                <div>
                  <span class="tag {s.kind === 'unassigned' ? 'warn' : 'bad'}">{KIND_LABEL[s.kind] || s.kind}</span>
                  <span class="tag info">需 {s.needed_model}</span>
                </div>
              </div>
              <div class="muted small" style="margin:6px 0">{s.message}</div>

              {#if s.assets_in_class.length > 0}
                <div class="small" style="margin:6px 0"><b>本班可用：</b></div>
                <div class="row">
                  {#each s.assets_in_class as a (a.asset_code + i)}
                    <button class="small success"
                            disabled={busy[`alloc-${s.student_id}-${a.asset_code}`]}
                            on:click={() => allocate(s.student_id, a.asset_code, `alloc-${s.student_id}-${a.asset_code}`)}>
                      {busy[`alloc-${s.student_id}-${a.asset_code}`] ? '处理中…' : `分配 ${a.asset_code}（${a.location_name}）`}
                    </button>
                  {/each}
                </div>
              {/if}

              {#if s.assets_elsewhere.length > 0}
                <div class="small" style="margin:8px 0 4px"><b>其他位置（需跨班调拨，双方确认后资产才移动）：</b></div>
                <div class="row">
                  {#each s.assets_elsewhere as a (a.asset_code + 'e' + i)}
                    <button class="small warn"
                            disabled={busy[`tr-${s.student_id}-${a.asset_code}`]}
                            on:click={() => createTransferFromSuggestion(s, a, `tr-${s.student_id}-${a.asset_code}`)}>
                      申请调拨 {a.asset_code}（{a.location_name}）
                    </button>
                  {/each}
                </div>
              {/if}

              {#if s.assets_in_class.length === 0 && s.assets_elsewhere.length === 0}
                <div class="small" style="color:var(--red)">暂无可分配的 {s.needed_model} 型号资产（损坏品不在此列），请等待调拨或采购。</div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>

  {:else if tab === 'assets'}
    <div class="card">
      <div class="section-title">
        <h2 style="font-size:16px">本地模拟扫码</h2>
      </div>
      <div class="row">
        <input placeholder="输入或扫入桌椅编号，如 DSK-009" bind:value={scanCode}
               on:keydown={(e) => e.key === 'Enter' && doScan()} style="width:260px" />
        <button class="primary" on:click={doScan}>模拟扫码</button>
        <select on:change={(e) => { scanCode = e.target.value; }}>
          <option value="">选择编号…</option>
          {#each assets as a (a.code)}
            <option value={a.code}>{a.code} · {a.model} · {a.location_name}</option>
          {/each}
        </select>
      </div>
      {#if scanResult}
        <div style="margin-top:12px;border:1px dashed var(--border);border-radius:8px;padding:12px;">
          <div class="row" style="justify-content:space-between">
            <div><code style="font-size:15px">{scanResult.code}</code> · {scanResult.model}
              <span class="tag {tag(scanResult.status).cls}">{tag(scanResult.status).text}</span>
            </div>
            <button class="small" on:click={() => (historyAsset = scanResult.code)}>查看履历</button>
          </div>
          <div class="muted small" style="margin:6px 0">
            位置：{scanResult.location_name} ·
            占用：{scanResult.student_name ? `${scanResult.student_name}（#${scanResult.assigned_student_id}）` : '无'} ·
            {#if scanResult.active_transfer_id}<span class="tag purple">在途调拨 #${scanResult.active_transfer_id}</span>{:else}无在途调拨{/if}
          </div>
          <div class="row">
            {#if scanResult.status === 'available'}
              <button class="small danger" on:click={() => setDamage(scanResult.code, true)}>标记损坏</button>
            {:else if scanResult.status === 'damaged'}
              <button class="small success" on:click={() => setDamage(scanResult.code, false)}>维修完成入库</button>
            {:else if scanResult.status === 'in_use' && scanResult.assigned_student_id}
              <button class="small warn" on:click={() => unassign(scanResult.assigned_student_id)}>收回该桌椅</button>
            {/if}
          </div>
        </div>
      {/if}
    </div>

    <div class="card">
      <div class="section-title"><h2 style="font-size:16px">全部资产</h2><button on:click={refresh} class="small">刷新</button></div>
      <table>
        <thead><tr><th>编号</th><th>型号</th><th>状态</th><th>位置</th><th>占用学生</th><th>操作</th></tr></thead>
        <tbody>
          {#each assets as a (a.code)}
            <tr>
              <td><code>{a.code}</code></td>
              <td>{a.model}</td>
              <td>
                <span class="tag {tag(a.status).cls}">{tag(a.status).text}</span>
                {#if a.active_transfer_id}<span class="tag purple">在途 #${a.active_transfer_id}</span>{/if}
              </td>
              <td>{a.location_name}</td>
              <td>{a.student_name || '—'}</td>
              <td class="row">
                <button class="small" on:click={() => (historyAsset = a.code)}>履历</button>
                {#if a.status === 'available'}
                  <button class="small danger" on:click={() => setDamage(a.code, true)}>损坏</button>
                {:else if a.status === 'damaged'}
                  <button class="small success" on:click={() => setDamage(a.code, false)}>修复</button>
                {:else if a.assigned_student_id}
                  <button class="small warn" on:click={() => unassign(a.assigned_student_id)}>收回</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

  {:else if tab === 'transfers'}
    <div class="card">
      <h2 style="font-size:16px;margin-bottom:10px">发起跨班调拨</h2>
      <div class="row">
        <input placeholder="桌椅编号（如 DSK-008）" bind:value={newTransfer.asset_code} style="width:170px" />
        <select bind:value={newTransfer.from_class_id}>
          <option value="">交出方…</option>
          {#each classes as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
        </select>
        <span>→</span>
        <select bind:value={newTransfer.to_class_id}>
          <option value="">接收方…</option>
          {#each classes as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
        </select>
        <button class="primary" on:click={createTransferManual}>发起调拨</button>
      </div>
      <p class="muted small">流程：发起 → <b>交出方放行</b> → <b>接收方确认</b>。两步完成前资产位置不变。后勤可代任一方操作。</p>
    </div>

    <div class="card">
      <div class="section-title"><h2 style="font-size:16px">调拨单</h2><button on:click={refresh} class="small">刷新</button></div>
      <table>
        <thead><tr><th>单号</th><th>桌椅</th><th>交出 → 接收</th><th>状态</th><th>操作</th></tr></thead>
        <tbody>
          {#each transfers as t (t.id)}
            <tr>
              <td>#{t.id}</td>
              <td><code>{t.asset_code}</code></td>
              <td>{t.from_class_name} → {t.to_class_name}</td>
              <td><span class="tag {tag(t.status).cls}">{tag(t.status).text}</span></td>
              <td class="row">
                {#if t.status === 'pending'}
                  <button class="small success" on:click={() => transferAction(t.id, 'release')}>① 交出方放行</button>
                  <button class="small danger" on:click={() => transferAction(t.id, 'reject')}>拒绝</button>
                {:else if t.status === 'released'}
                  <button class="small primary" on:click={() => transferAction(t.id, 'confirm')}>② 接收方确认（移动资产）</button>
                  <button class="small danger" on:click={() => transferAction(t.id, 'reject')}>接收方拒收</button>
                {:else}
                  <button class="small" on:click={() => (historyAsset = t.asset_code)}>查看资产履历</button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

  {:else if tab === 'overview'}
    <div class="grid cols-3" style="margin-bottom:16px">
      <div class="stat"><div class="num">{overview?.students ?? '…'}</div><div class="label">学生总数</div></div>
      <div class="stat"><div class="num">{overview?.in_use ?? '…'}</div><div class="label">使用中桌椅</div></div>
      <div class="stat"><div class="num">{overview?.available ?? '…'}</div><div class="label">空闲可分配</div></div>
      <div class="stat"><div class="num" style="color:var(--red)">{overview?.damaged ?? '…'}</div><div class="label">损坏</div></div>
      <div class="stat"><div class="num" style="color:var(--amber)">{overview?.mismatch_count ?? '…'}</div><div class="label">身高不匹配</div></div>
      <div class="stat"><div class="num" style="color:var(--purple)">{overview?.active_transfers ?? '…'}</div><div class="label">在途调拨</div></div>
    </div>

    <div class="card">
      <h2 style="font-size:16px;margin-bottom:10px">学生换班登记</h2>
      <p class="muted small">换班只改学生所属班级，桌椅留在原班、原分配与调拨履历完整保留，不会把资产算到新班。</p>
      <div class="row">
        <input placeholder="学生编号（如 1）" bind:value={moveForm.student_id} style="width:160px" />
        <select bind:value={moveForm.class_id}>
          <option value="">目标班级…</option>
          {#each classes?.length ? classes : [] as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
        </select>
        <button class="primary" on:click={moveStudent}>登记换班</button>
        <button class="small" on:click={async () => { const c = await api.get('/classes'); classes = c.classes; }}>载入班级</button>
      </div>
    </div>
  {/if}
</div>

{#if historyAsset}
  <HistoryModal assetCode={historyAsset} on:close={() => (historyAsset = null)} />
{/if}
