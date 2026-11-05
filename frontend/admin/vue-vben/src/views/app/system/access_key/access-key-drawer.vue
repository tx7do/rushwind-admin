<script lang="ts" setup>
import { computed, ref } from 'vue';

import { useVbenDrawer } from '@vben/common-ui';
import { useFormCloseGuard } from '#/composables/use-form-close-guard';
import { $t } from '@vben/locales';

import { notification } from 'ant-design-vue';

import { useVbenForm } from '#/adapter/form';
import { useCreateAccessKey, useUpdateAccessKey } from '#/api';

const { mutateAsync: createAccessKey } = useCreateAccessKey();
const { mutateAsync: updateAccessKey } = useUpdateAccessKey();

const data = ref();
const secretVisible = ref(false);
const createdSecret = ref('');

const getTitle = computed(() =>
  data.value?.create
    ? $t('ui.modal.create', { moduleName: $t('page.accessKey.moduleName') })
    : $t('ui.modal.update', { moduleName: $t('page.accessKey.moduleName') }),
);

const [BaseForm, baseFormApi] = useVbenForm({
  showDefaultActions: false,
  commonConfig: {
    componentProps: {
      class: 'w-full',
    },
  },
  schema: [
    {
      component: 'Input',
      fieldName: 'name',
      label: $t('page.accessKey.name'),
      componentProps: {
        placeholder: $t('ui.placeholder.input'),
        allowClear: true,
      },
      rules: 'required',
    },
    {
      component: 'RangePicker',
      fieldName: 'expiresAtRange',
      label: $t('page.accessKey.expiresAt'),
      componentProps: {
        showTime: true,
        placeholder: [$t('ui.placeholder.select'), $t('ui.placeholder.select')],
      },
    },
    {
      component: 'RadioGroup',
      fieldName: 'status',
      label: $t('page.accessKey.status'),
      defaultValue: 'ON',
      rules: 'selectRequired',
      componentProps: {
        optionType: 'button',
        buttonStyle: 'solid',
        options: [
          { label: $t('page.accessKey.statusOn'), value: 'ON' },
          { label: $t('page.accessKey.statusOff'), value: 'OFF' },
        ],
      },
    },
  ],
});

// 关闭守卫：遮罩/ESC/X/取消统一，脏数据先确认（docs/form_drawer_close_guard.md）
const closeGuard = useFormCloseGuard(baseFormApi);

const [Drawer, drawerApi] = useVbenDrawer({
  onBeforeClose: () => closeGuard.onBeforeClose(),
  onCancel() {
    drawerApi.close();
  },

  async onConfirm() {
    const validate = await baseFormApi.validate();
    if (!validate.valid) {
      return;
    }

    setLoading(true);

    const values = await baseFormApi.getValues();

    try {
      if (data.value?.create) {
        const expiresAtRange = (values.expiresAtRange ?? []) as string[];
        const resp = await createAccessKey({
          name: values.name,
          status: values.status,
          expiresAt: expiresAtRange[1] ?? undefined,
        });
        notification.success({
          message: $t('ui.notification.create_success'),
        });
        closeGuard.markClean();
        if (resp.secret) {
          // 一次性 Secret 展示：a-modal teleport 到 body，抽屉关闭后仍可见
          createdSecret.value = resp.secret;
          secretVisible.value = true;
        }
      } else {
        const expiresAtRange = (values.expiresAtRange ?? []) as string[];
        await updateAccessKey({
          id: data.value.row.id,
          values: {
            name: values.name,
            status: values.status,
            expiresAt: expiresAtRange[1] ?? undefined,
          },
        });
        notification.success({
          message: $t('ui.notification.update_success'),
        });
        closeGuard.markClean();
      }
    } catch {
      notification.error({
        message: data.value?.create
          ? $t('ui.notification.create_failed')
          : $t('ui.notification.update_failed'),
      });
    } finally {
      drawerApi.close();
      setLoading(false);
    }
  },

  async onOpenChange(isOpen) {
    if (isOpen) {
      data.value = drawerApi.getData<Record<string, any>>();
      await baseFormApi.setValues(data.value?.row ?? {});
      await closeGuard.syncBaseline();
      setLoading(false);
    }
  },
});

function copySecret() {
  navigator.clipboard?.writeText(createdSecret.value).catch((err: unknown) => {
    console.error('copy secret failed:', err);
  });
  notification.success({ message: $t('page.accessKey.secretCopied') });
}

function setLoading(loading: boolean) {
  drawerApi.setState({ loading });
}
</script>

<template>
  <Drawer :title="getTitle">
    <BaseForm />
  </Drawer>

  <a-modal
    v-model:open="secretVisible"
    :title="$t('page.accessKey.secretDialogTitle')"
    :cancel-button-props="{ style: { display: 'none' } }"
    :mask-closable="false"
    width="560px"
  >
    <a-alert type="warning" :message="$t('page.accessKey.secretDialogHint')" />
    <a-input class="mt-3" :value="createdSecret" readonly>
      <template #addonAfter>
        <a @click="copySecret">{{ $t('page.accessKey.secretCopied') }}</a>
      </template>
    </a-input>
  </a-modal>
</template>
