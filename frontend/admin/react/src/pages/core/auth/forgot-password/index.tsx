import { useNavigate } from 'react-router-dom';
import { Button, Form, Input, App } from 'antd';
import { useTranslation } from 'react-i18next';
import { useState } from 'react';
import { apiClient } from '@/api/client';
import AuthLayout from '@/components/bussiness/AuthLayout';

/**
 * 忘记密码页面：
 * 步骤 1 输入绑定的邮箱或手机号 → 发送验证码（邮箱走 SMTP，手机号走短信渠道）；
 * 步骤 2 输入验证码 + 新密码 → 重置成功跳登录页。
 */
const ForgotPassword = () => {
  const { t } = useTranslation('forgot-password');
  const navigate = useNavigate();
  const { message } = App.useApp();
  const [form] = Form.useForm();
  const [step, setStep] = useState<1 | 2>(1);
  const [identifier, setIdentifier] = useState('');
  const [sending, setSending] = useState(false);
  const [resetting, setResetting] = useState(false);

  const sendCode = async () => {
    const values = await form.validateFields(['identifier']);
    setSending(true);
    try {
      await apiClient.authenticationService.ForgotPassword({
        identifier: values.identifier,
      });
      setIdentifier(values.identifier);
      setStep(2);
      message.success(t('codeSent'));
    } catch (error: any) {
      message.error(error.message || t('sendFailed'));
    } finally {
      setSending(false);
    }
  };

  const resetByCode = async (values: { code: string; newPassword: string }) => {
    setResetting(true);
    try {
      await apiClient.authenticationService.ResetPasswordByCode({
        identifier,
        code: values.code,
        new_password: values.newPassword,
      });
      message.success(t('resetSuccess'));
      navigate('/auth/login');
    } catch (error: any) {
      message.error(error.message || t('resetFailed'));
    } finally {
      setResetting(false);
    }
  };

  return (
    <AuthLayout title={t('title')} description={t('subtitle')}>
      {step === 1 ? (
        <Form form={form} layout="vertical" onFinish={sendCode}>
          <Form.Item
            name="identifier"
            label={t('identifier')}
            rules={[{ required: true, message: t('requiredIdentifier') }]}
          >
            <Input placeholder={t('identifierPlaceholder')} size="large" />
          </Form.Item>
          <Button type="primary" size="large" block loading={sending} onClick={sendCode}>
            {t('sendCode')}
          </Button>
        </Form>
      ) : (
        <Form layout="vertical" onFinish={resetByCode}>
          <Form.Item
            name="code"
            label={t('code')}
            rules={[{ required: true, message: t('requiredCode') }]}
          >
            <Input placeholder={t('codePlaceholder')} size="large" />
          </Form.Item>
          <Form.Item
            name="newPassword"
            label={t('newPassword')}
            rules={[{ required: true, message: t('requiredNewPassword') }]}
          >
            <Input.Password placeholder={t('newPasswordPlaceholder')} size="large" />
          </Form.Item>
          <Button type="primary" size="large" block loading={resetting} onClick={() => form.submit()}>
            {t('reset')}
          </Button>
        </Form>
      )}

      <div className="mt-4 text-center">
        <Button type="link" size="small" onClick={() => navigate('/auth/login')}>
          {t('backToLogin')}
        </Button>
      </div>
    </AuthLayout>
  );
};

export default ForgotPassword;
