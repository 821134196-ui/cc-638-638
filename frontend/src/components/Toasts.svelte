<script>
  import { toasts } from '../toast.js';
</script>

<div class="toast-wrap">
  {#each $toasts as t (t.id)}
    <div class="toast {t.type}">{t.message}</div>
  {/each}
</div>
