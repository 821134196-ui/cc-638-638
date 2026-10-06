<script>
  import { onMount } from 'svelte';
  import { api, getUser, clearSession } from './api.js';
  import Login from './pages/Login.svelte';
  import TeacherView from './pages/TeacherView.svelte';
  import LogisticsView from './pages/LogisticsView.svelte';
  import Toasts from './components/Toasts.svelte';

  let user = getUser();

  function onLogin({ token, user: u }) {
    localStorage.setItem('deskflow_token', token);
    localStorage.setItem('deskflow_user', JSON.stringify(u));
    user = u;
  }

  function logout() {
    clearSession();
    user = null;
  }

  let classes = [];
  onMount(async () => {
    if (user) {
      try {
        classes = await api.get('/classes');
      } catch (e) {
        if (e.status === 401) logout();
      }
    }
  });

  $: className = (id) => classes.find((c) => c.id === id)?.name || `班级#${id}`;
</script>

<Toasts />

{#if !user}
  <Login on:login={(e) => onLogin(e.detail)} />
{:else}
  <div class="layout">
    <header>
      <div class="brand">🪑 课桌椅匹配与跨班调拨系统</div>
      <div class="who">
        <span>{user.display_name}</span>
        <span class="badge {user.role === 'logistics' ? 'warn' : 'info'}">
          {user.role === 'logistics' ? '后勤' : '班主任'}
        </span>
        {#if user.role === 'teacher' && user.class_id}
          <span class="muted">{className(user.class_id)}</span>
        {/if}
        <button class="tiny" on:click={logout}>退出登录</button>
      </div>
    </header>

    <main>
      {#if user.role === 'logistics'}
        <LogisticsView {classes} />
      {:else if user.class_id}
        <TeacherView classId={user.class_id} className={className(user.class_id)} />
      {:else}
        <div class="card">该班主任账号未绑定班级，请联系管理员。</div>
      {/if}
    </main>
  </div>
{/if}

<style>
  .layout { max-width: 1180px; margin: 0 auto; padding: 0 18px 40px; }
  header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 16px 2px; gap: 12px; flex-wrap: wrap;
  }
  .brand { font-weight: 700; font-size: 17px; }
  .who { display: flex; align-items: center; gap: 10px; }
</style>
