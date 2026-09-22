<template>
  <Fallback status="404" />
</template>

<script lang="ts" setup>
import Fallback from "./fallback.vue";

defineOptions({ name: "Fallback404" });
</script>
