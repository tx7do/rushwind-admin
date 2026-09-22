<script lang="ts" setup>
import type { VbenFormSchema } from '@vben/common-ui';

import { computed, h, onMounted, ref } from 'vue';

import { AuthenticationLogin, z } from '@vben/common-ui';
import { $t } from '@vben/locales';

import { useAuthStore } from '#/stores';
import { fetchGenerateCaptcha } from '#/api/composables';

defineOptions({ name: 'Login' });

const authStore = useAuthStore();

// 验证码状态
const captchaId = ref('');
const captchaImage = ref('');
const captchaLoading = ref(false);

async function refreshCaptcha() {
  captchaLoading.value = true;
  try {
    const resp = await fetchGenerateCaptcha();
    captchaId.value = resp.captchaId ?? '';
    captchaImage.value = resp.imageBase64 ?? '';
  } catch {
    // 验证码获取失败不阻断页面
  } finally {
    captchaLoading.value = false;
  }
}

onMounted(() => {
  refreshCaptcha();
});

// 验证码图片渲染函数（响应式读取 captchaImage / captchaLoading）
// 作为函数式组件传入 suffix，由 VbenRenderContent 通过 h() 渲染
const renderCaptchaImage = () =>
  h(
    'div',
    {
      title: $t('authentication.captchaRefresh'),
      onClick: () => {
        if (!captchaLoading.value) refreshCaptcha();
      },
      style: {
        height: '36px',
        width: '110px',
        flexShrink: '0',
        cursor: 'pointer',
        borderRadius: '6px',
        overflow: 'hidden',
        border: '1px solid var(--border)',
        background: 'var(--input)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      },
    },
    captchaImage.value
      ? [
          h('img', {
            src: captchaImage.value,
            alt: 'captcha',
            style: {
              height: '100%',
              width: '100%',
              objectFit: 'cover',
            },
          }),
        ]
      : [
          h(
            'span',
            { style: { color: 'var(--muted-foreground)', fontSize: '12px' } },
            captchaLoading.value ? '...' : $t('authentication.captchaRefresh'),
          ),
        ],
  );

const formSchema = computed((): VbenFormSchema[] => {
  return [
    {
      component: 'VbenInput',
      componentProps: {
        placeholder: $t('authentication.tenantCode'),
      },
      fieldName: 'tenant_code',
      label: $t('authentication.tenantCode'),
      rules: z.optional(z.string()),
    },
    {
      component: 'VbenInput',
      componentProps: {
        placeholder: $t('authentication.usernameTip'),
      },
      dependencies: {
        trigger(values) {
          if (values.selectAccount) {
          }
        },
        triggerFields: ['selectAccount'],
      },
      fieldName: 'username',
      label: $t('authentication.username'),
      rules: z.string().min(1, { message: $t('authentication.usernameTip') }),
    },
    {
      component: 'VbenInputPassword',
      componentProps: {
        placeholder: $t('authentication.password'),
      },
      fieldName: 'password',
      label: $t('authentication.password'),
      rules: z.string().min(1, { message: $t('authentication.passwordTip') }),
    },
    {
      component: 'VbenInput',
      componentProps: {
        placeholder: $t('authentication.captchaTip'),
        autocomplete: 'off',
        class: 'w-auto flex-1 min-w-0',
      },
      fieldName: 'captchaValue',
      label: $t('authentication.captcha'),
      rules: z.string().min(1, { message: $t('authentication.captchaTip') }),
      suffix: renderCaptchaImage,
    },
  ];
});

// 包装 authLogin：提交时把 captchaId 一并传入
async function handleSubmit(values: Record<string, any>) {
  const result = await authStore.authLogin({
    ...values,
    captchaId: captchaId.value,
  });
  // 登录失败时刷新验证码
  if (!result?.userInfo) {
    refreshCaptcha();
  }
}
</script>

<template>
  <!-- 对齐 react：标题/描述在卡片外，登录表单包一张 24px 圆角卡片（边框 + 主色柔影） -->
  <div>
    <div class="mb-7">
      <h2
        class="text-foreground mb-3 text-3xl font-bold leading-9 tracking-tight lg:text-4xl"
      >
        {{ $t('authentication.welcomeBack') }} 👋🏻
      </h2>
      <p class="text-muted-foreground text-sm lg:text-md">
        {{ $t('authentication.loginSubtitle') }}
      </p>
    </div>

    <div
      class="rounded-3xl border border-border bg-card p-8 shadow-[0_12px_40px_-8px_rgba(0,107,230,0.18)]"
    >
      <AuthenticationLogin
        :form-schema="formSchema"
        :loading="authStore.loginLoading"
        :show-code-login="false"
        :show-forget-password="false"
        :show-qrcode-login="false"
        :show-register="false"
        :show-third-party-login="false"
        @submit="handleSubmit"
      >
        <!-- 内置标题已移到卡片外，置空默认标题块 -->
        <template #title><span class="hidden"></span></template>
      </AuthenticationLogin>
    </div>
  </div>
</template>
