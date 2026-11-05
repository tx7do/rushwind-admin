<template>
  <div
    v-if="showPagination"
    class="pro-pagination"
    :class="{
      'pro-pagination--hidden': hideOnSinglePage && (total ?? 0) <= (pageSize ?? DEFAULT_PAGE_SIZE),
    }"
  >
    <ElPagination
      v-model:current-page="currentPage"
      v-model:page-size="pageSize"
      :page-sizes="pageSizes"
      :total="total"
      :background="background"
      :disabled="disabled"
      :layout="computedLayout"
      :pager-count="pagerCount"
      :small="small"
      v-bind="$attrs"
      @current-change="handleCurrentChange"
      @size-change="handleSizeChange"
      @prev-click="handlePrevClick"
      @next-click="handleNextClick"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { ElPagination } from "element-plus";
import { DEFAULT_CURRENT_PAGE, DEFAULT_PAGE_SIZE, DEFAULT_PAGE_SIZES } from "../constants";
import type { ProPaginationProps, PaginationEmits } from "./types";

defineOptions({ inheritAttrs: false });

const props = withDefaults(defineProps<ProPaginationProps>(), {
  currentPage: DEFAULT_CURRENT_PAGE,
  pageSize: DEFAULT_PAGE_SIZE,
  pageSizes: () => DEFAULT_PAGE_SIZES,
  total: 0,
  background: true,
  disabled: false,
  showTotal: true,
  showSizes: true,
  showJump: false,
  pagerCount: 7,
  small: false,
  hideOnSinglePage: false,
});

const emit = defineEmits<PaginationEmits>();

const currentPage = ref(props.currentPage);
const pageSize = ref(props.pageSize);

// 自动计算 layout：左侧 total + sizes，右侧翻页器
const computedLayout = computed(() => {
  if (props.layout) return props.layout;
  const left: string[] = ["total"];
  if (props.showSizes) left.push("sizes");
  const right: string[] = ["prev", "pager", "next"];
  if (props.showJump) right.push("jumper");
  return left.join(", ") + " -> " + right.join(", ");
});

const showPagination = computed(() => {
  if (props.hideOnSinglePage) {
    return props.total > props.pageSize;
  }
  return props.total > 0;
});

function handleCurrentChange(val: number) {
  currentPage.value = val;
  emit("update:modelValue", { currentPage: val, pageSize: pageSize.value });
  emit("current-change", val);
}

function handleSizeChange(val: number) {
  pageSize.value = val;
  currentPage.value = 1;
  emit("update:modelValue", { currentPage: 1, pageSize: val });
  emit("size-change", val);
}

function handlePrevClick(val: number) {
  emit("prev-click", val);
}

function handleNextClick(val: number) {
  emit("next-click", val);
}

// 监听外部变化
watch(
  () => props.currentPage,
  (val) => {
    currentPage.value = val;
  }
);
watch(
  () => props.pageSize,
  (val) => {
    pageSize.value = val;
  }
);

// 暴露方法
defineExpose({
  currentPage,
  pageSize,
  reset: () => {
    currentPage.value = 1;
    pageSize.value = props.pageSize;
  },
  setCurrentPage: (page: number) => {
    currentPage.value = page;
  },
  setPageSize: (size: number) => {
    pageSize.value = size;
  },
});
</script>

<style scoped lang="scss">
.pro-pagination {
  display: flex;
  align-items: center;
  justify-content: space-between;
  /* 上 16px = §2.6「表格 → 分页器」；下 0：分页器到卡片边的留白由 .pro-page__content
     的 20px 内缩唯一提供（原 `12px 0` 与容器 12px 叠成上 12/下 24，两个来源各写一份）。
     暗色在 styles/_dark-mode.scss 里有同一条的 !important 覆写，两处必须一起改。 */
  padding: 16px 0 0;
  background-color: var(--el-bg-color);
  margin-top: auto; // 贴底 ProPage 容器

  &--hidden {
    display: none;
  }

  :deep(.el-pagination) {
    width: 100%;

    // 左侧区域（total + sizes）
    .el-pagination__total,
    .el-pagination__sizes {
      margin-right: 0;
    }

    .el-pagination__jump {
      margin-left: 0;
    }
  }
}
</style>
