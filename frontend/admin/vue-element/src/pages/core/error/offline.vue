<template>
  <Fallback status="offline" />
</template>

<script lang="ts" setup>
import Fallback from "./fallback.vue";

defineOptions({ name: "FallbackOffline" });
</script>
