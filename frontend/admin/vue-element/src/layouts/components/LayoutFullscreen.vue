<template>
  <div class="fullscreen-trigger" @click="toggle">
    <SvgIcon :icon="iconIsFullscreen ? 'fullscreen-exit' : 'fullscreen'" />
  </div>
</template>

<script setup lang="ts">
import SvgIcon from "@/components/SvgIcon/index.vue";

const { toggle } = useFullscreen();

// 图标态不直接用 vueuse 的 isFullscreen：v14 的实现只在自家 enter/exit 里赋值、
// 不监听 fullscreenchange，用户按 ESC 退出后图标会停在「退出全屏」。与 react 顶栏
// /列表工具栏同一定式：跟随浏览器真实全屏状态（v10 的 vueuse 有事件监听，v14 没有）。
const iconIsFullscreen = ref(document.fullscreenElement != null);

function syncFullscreen() {
  iconIsFullscreen.value = document.fullscreenElement != null;
}

onMounted(() => document.addEventListener("fullscreenchange", syncFullscreen));
onBeforeUnmount(() => document.removeEventListener("fullscreenchange", syncFullscreen));
</script>

<style lang="scss" scoped>
.fullscreen-trigger {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
}
</style>
