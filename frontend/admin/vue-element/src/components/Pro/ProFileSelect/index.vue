<template>
  <input
    ref="inputRef"
    type="file"
    class="pro-file-select"
    :accept="accept"
    :multiple="multiple"
    @change="handleChange"
  />
</template>

<script setup lang="ts">
import { ref } from "vue";
import type { ProFileSelectProps } from "./types";

withDefaults(defineProps<ProFileSelectProps>(), {
  accept: undefined,
  multiple: false,
});

const emit = defineEmits<{
  select: [files: File[]];
}>();

const inputRef = ref<HTMLInputElement>();

function handleChange(event: Event) {
  const target = event.target as HTMLInputElement;
  const files = target.files ? Array.from(target.files) : [];
  if (files.length > 0) {
    emit("select", files);
  }
  // 清空 value，确保同一文件可以重复选择
  target.value = "";
}

function open() {
  inputRef.value?.click();
}

defineExpose({ open });
</script>

<style lang="scss" scoped>
.pro-file-select {
  display: none;
}
</style>
