<script setup lang="ts">
import type { PropType } from 'vue';

import { CheckboxGroup } from 'ant-design-vue';

/**
 * 字段权限卡片（角色抽屉）：对齐 react 基准——「字段权限」段头顶格左对齐 +
 * 圆角边框卡内 提示语 + User/Role 两组竖排勾选。
 * modelValue 形态 { user: string[], role: string[] }，提交前由抽屉折算成
 * fieldPermissions 多资源数组（User/Role 即资源标识，与后端 proto 一致）。
 */
interface Option {
  label: string;
  value: string;
}
interface Props {
  hint?: string;
  roleOptions?: Option[];
  title?: string;
  userOptions?: Option[];
}
const props = withDefaults(defineProps<Props>(), {
  hint: '',
  roleOptions: () => [],
  title: '',
  userOptions: () => [],
});

const model = defineModel({
  type: Object as PropType<{ role: string[]; user: string[] }>,
  default: () => ({ role: [], user: [] }),
});

function setGroup(resource: 'role' | 'user', values: string[]) {
  model.value = { ...model.value, [resource]: values };
}
</script>

<template>
  <div class="w-full">
    <div class="mb-1 text-sm font-medium">{{ title }}</div>
    <div class="rounded-md border border-border p-3">
      <p class="text-muted-foreground mb-2 text-xs leading-5">{{ hint }}</p>
      <p class="text-foreground/70 mb-1 text-xs font-medium">User</p>
      <CheckboxGroup
        :options="props.userOptions"
        :value="model.user"
        class="flex flex-col items-start gap-1.5"
        @update:value="(vals: any) => setGroup('user', vals)"
      />
      <p class="text-foreground/70 mb-1 mt-3 text-xs font-medium">Role</p>
      <CheckboxGroup
        :options="props.roleOptions"
        :value="model.role"
        class="flex flex-col items-start gap-1.5"
        @update:value="(vals: any) => setGroup('role', vals)"
      />
    </div>
  </div>
</template>
