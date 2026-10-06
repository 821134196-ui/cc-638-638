<script>
  import { onMount } from 'svelte';
  import { api, toast, user, STATUS_LABEL, KIND_LABEL } from '../lib/api.js';

  export let logout;

  let tab = 'dashboard';
  let dash = null;
  let students = [];
  let transfers = [];
  let assets = [];
  let loading = false;

  const classId = () => $user.class_id;

  async function refresh() {
    loading = true;
    try {
      const cid = classId();
      if (tab === 'dashboard' || tab === 'students') {
        const d = await api.get(`/classes/${cid}/dashboard`);
        dash = d;
        const s = await api.get(`/classes/${cid}/students`);
        students = s.students;
      }
      if (tab === 'transfers') {
        const t = await api.get('/transfers');
        transfers = t.transfers;
      }
      if (tab === 'assets') {
        const a = await api.get('/assets');
        assets = a.assets;
      }
    } catch (e) {
      toast(e.message, 'err');
    } finally {
      loading = false;
    }
  }

  onMount(refresh);

  let editing = null; // {id, name, height_cm, saving}
  function startEdit(s) {
    editing = { id: s.id, name: s.name, height_cm: s.height_cm, saving: false };
  }
  async function saveHeight() {
    if (!editing.height_cm || editing.height_cm < 80 || editing.height_cm > 220) {
      return toast('身高需在 80~220cm 之间', 'err');
    }
    editing.saving = true;
    try {
      const r = await api.patch(`/students/${editing.id}/height`, { height_cm: Number(editing.height_cm) });
      toast(`身高已更新，建议型号：${r.needed_model}。${r.notice}`, 'ok', 6000);
      editing = null;
      await refresh();
    } catch (e) {
      toast(e.message, 'err');
    } finally {
      editing.saving = false;
    }
  }

  async function actTransfer(id, action) {
    try {
      const r = await api.post(`/transfers/${id}/${action}`, {});
      toast(r.notice || '操作成功', 'ok', 5000);
      await refresh();
    } catch (e) {
      toast(e.message, 'err');
    }
  }

  function tagForStatus(s) {
    return STATUS_LABEL[s] || { text: s, cls: 'gray' };
  }
  function canRelease(t) { return t.status === 'pending' && t.from_class_id === classId(); }
  function canConfirm(t) { return t.status === 'released' && t.to_class_id === classId(); }
  function canReject(t) {
    if (t.status === 'pending') return true;
    return t.status === 'released' && t.to_class_id === classId();
  }
</script>

<div style="max-width:1100px;margin:0 auto;padding:20px;">
  <div class="row" style="justify-content:space-between;margin-bottom:16px">
    <div>
      <h1 style="font-size:20px">班级工作台</h1>
      <span class="muted">{$user.display_name}{#if dash} · {dash.class.name}{/if}</span>
    </div>
    <button on:click={logout}>退出登录</button>
  </div>

  <div class="tabs">
    <button class={tab === 'dashboard' ? 'active' : ''} on:click={() => { tab = 'dashboard'; refresh(); }}>班级总览</button>
    <button class={tab === 'students' ? 'active' : ''} on:click={() => { tab = 'students'; refresh(); }}>学生与身高</button>
    <button class={tab === 'transfers' ? 'active' : ''} on:click={() => { tab = 'transfers'; refresh(); }}>跨班调拨确认</button>
    <button class={tab === 'assets' ? 'active' : ''} on:click={() => { tab = 'assets'; refresh(); }}>本班资产</button>
  </div>

  {#if tab === 'dashboard' && dash}
    <div class="grid cols-4" style="margin-bottom:16px">
      <div class="stat"><div class="num" style="color:var(--amber)">{dash.pending_swap.length}</div><div class="label">待调换</div></div>
      <div class="stat"><div class="num" style="color:var(--green)">{dash.assigned.length}</div><div class="label">已匹配分配</div></div>
      <div class="stat"><div class="num">{dash.available.length}</div><div class="label">本班空闲</div></div>
      <div class="stat"><div class="num" style="color:var(--red)">{dash.damaged.length}</div><div class="label">损坏</div></div>
    </div>

    <div class="card">
      <div class="section-title"><h2 style="font-size:16px">待调换学生</h2><button on:click={refresh} class="small">刷新</button></div>
      {#if dash.pending_swap.length === 0}
        <p class="muted">全班桌椅高度均匹配 🎉</p>
      {:else}
        <table>
          <thead><tr><th>学生</th><th>身高</th><th>现状</th><th>需要型号</th><th>问题</th></tr></thead>
          <tbody>
            {#each dash.pending_swap as p (p.student_id)}
              <tr>
                <td>{p.student_name}</td>
                <td>{p.height_cm} cm</td>
                <td>
                  {#if p.current_asset_code}
                    <code>{p.current_asset_code}</code> · {p.current_model}
                  {:else}<span class="tag gray">无桌椅</span>{/if}
                </td>
                <td><span class="tag info">{p.needed_model}</span></td>
                <td><span class="tag {p.kind === 'unassigned' ? 'warn' : 'bad'}">{KIND_LABEL[p.kind] || p.kind}</span>
                    <span class="muted small">{p.message}</span></td>
              </tr>
            {/each}
          </tbody>
        </table>
        <p class="muted small" style="margin-top:10px">调换由后勤核实现有资产后办理；系统建议不会直接改变任何桌椅归属。</p>
      {/if}
    </div>

    <div class="card">
      <h2 style="font-size:16px;margin-bottom:10px">损坏桌椅（不参与分配）</h2>
      {#if dash.damaged.length === 0}
        <p class="muted">暂无损坏记录</p>
      {:else}
        <div class="row">
          {#each dash.damaged as a (a.code)}
            <span class="tag bad"><code>{a.code}</code> · {a.model}</span>
          {/each}
        </div>
      {/if}
    </div>

  {:else if tab === 'students'}
    <div class="card">
      <div class="section-title"><h2 style="font-size:16px">本班学生</h2><button on:click={refresh} class="small">刷新</button></div>
      <table>
        <thead><tr><th>姓名</th><th>身高</th><th>现用桌椅</th><th>型号</th><th>匹配状态</th><th></th></tr></thead>
        <tbody>
          {#each students as s (s.id)}
            <tr>
              <td>{s.name}</td>
              <td>{s.height_cm} cm</td>
              <td>{s.asset_code ? s.asset_code : '—'}</td>
              <td>{s.asset_model || '—'}</td>
              <td>
                {#if s.match_status === 'matched'}
                  <span class="tag ok">匹配</span>
                {:else if s.match_status === 'unassigned'}
                  <span class="tag warn">暂无桌椅</span>
                {:else}
                  <span class="tag bad">{KIND_LABEL[s.match_status] || s.match_status}</span>
                {/if}
              </td>
              <td><button class="small" on:click={() => startEdit(s)}>更新身高</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

  {:else if tab === 'transfers'}
    <div class="card">
      <div class="section-title"><h2 style="font-size:16px">与本班相关的跨班调拨</h2><button on:click={refresh} class="small">刷新</button></div>
      {#if transfers.length === 0}
        <p class="muted">暂无调拨单</p>
      {:else}
        <table>
          <thead><tr><th>单号</th><th>桌椅</th><th>交出方</th><th>接收方</th><th>状态</th><th>操作</th></tr></thead>
          <tbody>
            {#each transfers as t (t.id)}
              <tr>
                <td>#{t.id}</td>
                <td><code>{t.asset_code}</code></td>
                <td class={t.from_class_id === classId() ? '' : 'muted'}>{t.from_class_name}{t.from_class_id === classId() ? '（本班）' : ''}</td>
                <td class={t.to_class_id === classId() ? '' : 'muted'}>{t.to_class_name}{t.to_class_id === classId() ? '（本班）' : ''}</td>
                <td><span class="tag {tagForStatus(t.status).cls}">{tagForStatus(t.status).text}</span></td>
                <td class="row">
                  {#if canRelease(t)}
                    <button class="small success" on:click={() => actTransfer(t.id, 'release')}>交出方放行</button>
                  {/if}
                  {#if canConfirm(t)}
                    <button class="small primary" on:click={() => actTransfer(t.id, 'confirm')}>接收确认（资产入库）</button>
                  {/if}
                  {#if canReject(t)}
                    <button class="small danger" on:click={() => actTransfer(t.id, 'reject')}>拒绝</button>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
        <p class="muted small" style="margin-top:10px">
          调拨需交出方与接收方依次确认；仅当接收方点击“确认”后，桌椅实物位置才更新到本班。
        </p>
      {/if}
    </div>

  {:else if tab === 'assets'}
    <div class="card">
      <div class="section-title"><h2 style="font-size:16px">本班位置上的桌椅</h2><button on:click={refresh} class="small">刷新</button></div>
      <table>
        <thead><tr><th>编号</th><th>型号</th><th>状态</th><th>占用学生</th></tr></thead>
        <tbody>
          {#each assets as a (a.code)}
            <tr>
              <td><code>{a.code}</code></td>
              <td>{a.model}</td>
              <td><span class="tag {tagForStatus(a.status).cls}">{tagForStatus(a.status).text}</span></td>
              <td>{a.student_name || '—'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

{#if editing}
  <div style="position:fixed;inset:0;background:rgba(20,30,45,.35);display:grid;place-items:center"
       on:click={() => (editing = null)} on:keydown={() => {}}>
    <div class="card" style="width:340px" on:click|stopPropagation on:keydown|stopPropagation>
      <h2 style="font-size:16px;margin-bottom:12px">更新身高 · {editing.name}</h2>
      <label class="field">
        <span>身高（cm）</span>
        <input type="number" min="80" max="220" bind:value={editing.height_cm} />
      </label>
      <p class="muted small">更新后系统按身高重新给出型号建议，不会自动调换桌椅。</p>
      <div class="row" style="justify-content:flex-end">
        <button on:click={() => (editing = null)}>取消</button>
        <button class="primary" disabled={editing.saving} on:click={saveHeight}>保存</button>
      </div>
    </div>
  </div>
{/if}
