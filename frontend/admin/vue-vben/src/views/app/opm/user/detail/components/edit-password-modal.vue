<script lang="ts" setup>
import { ref } from 'vue';

import { useVbenDrawer } from '@vben/common-ui';
import { $t } from '@vben/locales';

import { notification } from 'ant-design-vue';

import { useVbenForm } from '#/adapter/form';
import { useEditUserPassword } from '#/api';

const data = ref();

const { mutateAsync: editUserPassword } = useEditUserPassword();

const [BaseForm, baseFormApi] = useVbenForm({
  showDefaultActions: false,
  // 所有表单项共用，可单独在表单内覆盖
  commonConfig: {
    // 所有表单项
    componentProps: {
      class: 'w-full',
    },
  },
  schema: [
    {
      component: 'VbenInputPassword',
      fieldName: 'new_password',
      label: $t('page.user.form.newPassword'),
      componentProps: {
        passwordStrength: true,
        placeholder: $t('ui.placeholder.input'),
      },
      rules: 'required',
    },
    {
      component: 'VbenInputPassword',
      fieldName: 'confirm_password',
      label: $t('page.user.form.confirmPassword'),
      componentProps: {
        placeholder: $t('ui.placeholder.input'),
      },
      rules: 'required',
    },
  ],
});

const [Drawer, drawerApi] = useVbenDrawer({
  class: 'w-[520px]',
  onCancel() {
    drawerApi.close();
  },

  async onConfirm() {

    // 校验输入的数据
    const validate = await baseFormApi.validate();
    if (!validate.valid) {
      return;
    }

    setLoading(true);

    // 获取表单数据
    const values = await baseFormApi.getValues();

    if (values.new_password !== values.confirm_password) {
      notification.error({
        message: $t('ui.notification.password_mismatch'),
      });

      setLoading(false);
      drawerApi.close();
      return;
    }

    try {
      await editUserPassword({
        userId: data.value?.userId,
        newPassword: values.new_password,
      });

      setLoading(false);
      drawerApi.close();

      notification.success({
        message: $t('ui.notification.update_status_success'),
      });
    } catch {
      setLoading(false);

      notification.error({
        message: $t('ui.notification.update_status_failed'),
      });
    }
  },

  onOpenChange(isOpen: boolean) {
    if (isOpen) {
      // 获取传入的数据
      data.value = drawerApi.getData<any>();

      setLoading(false);

    }
  },
});

function setLoading(loading: boolean) {
  drawerApi.setState({ confirmLoading: loading });
}
</script>

<template>
  <Drawer :title="$t('page.user.button.editPassword')">
    <BaseForm />
  </Drawer>
</template>
