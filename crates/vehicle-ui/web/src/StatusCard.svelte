<script lang="ts">
  let {
    label,
    value,
    detail,
    icon,
    tone,
    primary = false,
  }: {
    label: string;
    value: string;
    detail: string;
    icon: string;
    tone: "current" | "warning" | "danger" | "neutral";
    primary?: boolean;
  } = $props();
</script>

<article class:primary class="status-card status-card--{tone}">
  <div class="status-card__head">
    <h3>{label}</h3>
    <span aria-hidden="true">{icon}</span>
  </div>
  <p class="status-card__value">{value}</p>
  <p class="status-card__detail">{detail}</p>
</article>
