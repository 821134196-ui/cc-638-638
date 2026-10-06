<script>
  import { createEventDispatcher } from 'svelte';
  import { api } from '../api.js';
  import { toast } from '../toast.js';

  const dispatch = createEventDispatcher();
  let username = '';
  let password = '';
  let loading = false;

  const accounts = [
    { label: '后勤张老师', username: 'logistics', password: 'admin1234' },
    { label: '一(1)班 王老师', username: 'teacher1', password: 'pass1234' },
    { label: '一(2)班 李老师', username: 'teacher2', password: 'pass1234' },
    { label: '二(1)班 赵老师', username: 'teacher3', password: 'pass1234' },
  ];

  async function submit() {
    if (!username || !password) return;
    loading = true;
    try {
      const data = await api.post('/auth/login', { username, password });
      dispatch('login', data);
    } catch (e) {
      toast.error(e.message);
    } finally {
      loading = false;
    }
  }

  function fill(a) {
    username = a.username;
    password = a.password;
  }
</script>

<div class="login-wrap">
  <div class="login-card">
    <div class="logo">🪑</div>
    <h1>课桌椅高度匹配与跨班调拨系统</h1>
    <p class="muted sub">班主任录入身高 · 后勤核实调配 · 跨班调拨双方确认</p>

    <form on:submit|preventDefault={submit}>
      <label>用户名
        <input bind:value={username} placeholder="如 logistics" autocomplete="username" />
      </label>
      <label>口令
        <input bind:value={password} type="password" placeholder="请输入口令"
               autocomplete="current-password" />
      </label>
      <button class="primary" type="submit" disabled={loading} style="width:100%;margin-top:6px">
        {loading ? '登录中…' : '登 录'}
      </button>
    </form>

    <div class="quick">
      <div class="muted quick-title">演示账号（点击填充）</div>
      {#each accounts as a}
        <button class="tiny" on:click={() => fill(a)}>{a.label}</button>
      {/each}
    </div>
  </div>
</div>

<style>
  .login-wrap {
    min-height: 100vh; display: flex; align-items: center; justify-content: center;
    padding: 24px;
    background: linear-gradient(135deg, #dbeafe 0%, #f1f5f9 55%);
  }
  .login-card {
    width: 420px; max-width: 100%;
    background: #fff; border: 1px solid var(--border); border-radius: 16px;
    padding: 30px 32px; box-shadow: 0 20px 50px rgba(15,23,42,.10);
  }
  .logo { font-size: 40px; }
  h1 { font-size: 18px; margin: 8px 0 4px; }
  .sub { margin: 0 0 20px; font-size: 13px; }
  form label { display: block; font-size: 13px; color: var(--muted); margin-bottom: 12px; }
  form input { width: 100%; margin-top: 5px; }
  .quick { margin-top: 18px; border-top: 1px dashed var(--border); padding-top: 14px; }
  .quick-title { font-size: 12px; margin-bottom: 8px; }
  .quick button { margin: 0 6px 6px 0; }
</style>
