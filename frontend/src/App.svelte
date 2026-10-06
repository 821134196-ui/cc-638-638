<script>
  import { onMount } from 'svelte';
  import { user, loadMe, api, toast } from './lib/api.js';
  import Login from './components/Login.svelte';
  import FacultyView from './components/FacultyView.svelte';
  import LogisticsView from './components/LogisticsView.svelte';
  import Toasts from './components/Toasts.svelte';

  onMount(loadMe);

  async function logout() {
    try { await api.post('/auth/logout'); } catch {}
    user.set(null);
    toast('已退出登录', 'info');
  }
</script>

<Toasts />

{#if !$user}
  <Login />
{:else if $user.role === 'logistics'}
  <LogisticsView {logout} />
{:else}
  <FacultyView {logout} />
{/if}
