<script>
  import { api, toast, user } from '../lib/api.js';
  let username = '';
  let password = '';
  let busy = false;

  async function submit() {
    if (!username || !password) return toast('请输入账号和密码', 'err');
    busy = true;
    try {
      const r = await api.post('/auth/login', { username, password });
      user.set(r.user);
      toast(`欢迎，${r.user.display_name}`, 'ok');
    } catch (e) {
      toast(e.message, 'err');
    } finally {
      busy = false;
    }
  }

  function fill(u) {
    username = u; password = u;
  }
</script>

<div class="login-wrap">
  <div class="card login-card">
    <h1>课桌椅高度匹配与跨班调拨系统</h1>
    <p class="muted" style="margin:6px 0 18px">小学后勤资产调拨平台</p>
    <form on:submit|preventDefault={submit}>
      <label class="field">
        <span>账号</span>
        <input bind:value={username} placeholder="如 faculty1 / logistics" autocomplete="username" />
      </label>
      <label class="field">
        <span>密码</span>
        <input type="password" bind:value={password} autocomplete="current-password" />
      </label>
      <button class="primary" style="width:100%" disabled={busy}>{busy ? '登录中…' : '登 录'}</button>
    </form>
    <div class="muted small" style="margin-top:14px; line-height:1.9">
      演示账号（密码同账号）：<br />
      <button class="small" on:click={() => fill('faculty1')}>faculty1 三年1班班主任</button>
      <button class="small" on:click={() => fill('faculty2')}>faculty2 三年2班</button>
      <button class="small" on:click={() => fill('faculty3')}>faculty3 四年1班</button><br />
      <button class="small" on:click={() => fill('logistics')}>logistics 后勤</button>
    </div>
  </div>
</div>
