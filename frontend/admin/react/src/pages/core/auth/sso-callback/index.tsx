import { useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { Result, Spin, Typography } from 'antd';
import ContentContainer from '@/layouts/components/PageContainer/ContentContainer';
import { completeSsoLogin } from '@/api/hooks/sso';

const { Text } = Typography;

/**
 * OIDC SSO 回调页：IdP 授权后重定向回 /auth/sso/callback?code=...&state=...，
 * 本页把 code+state 提交后端换本系统 JWT，成功后进首页；失败展示原因并给返回入口。
 */
const SsoCallback: React.FC = () => {
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const ranRef = useRef(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // StrictMode 下 effect 双跑：登录是幂等的（state 单次有效），
    // 但二次调用必然因 state 失效而报错，故用 ref 保证只提交一次
    if (ranRef.current) return;
    ranRef.current = true;

    const code = searchParams.get('code') || '';
    const state = searchParams.get('state') || '';

    if (!code || !state) {
      setError('missing code or state');
      return;
    }

    completeSsoLogin(code, state, () => {
      navigate('/', { replace: true });
    }).catch((err: any) => {
      // 原始错误必须留在控制台：展示给用户的是归一化文案
      console.error('sso callback login failed', err);
      setError(err?.message || 'SSO login failed');
    });
  }, [searchParams, navigate]);

  if (error) {
    return (
      <ContentContainer heightMode="auto" padding="24px">
        <Result
          status="error"
          title="SSO 登录失败"
          subTitle={
            <Text type="secondary">{error}</Text>
          }
          extra={
            <a onClick={() => navigate('/auth/login', { replace: true })}>
              返回登录页
            </a>
          }
        />
      </ContentContainer>
    );
  }

  return (
    <ContentContainer heightMode="auto" padding="24px">
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', padding: 80, gap: 16 }}>
        <Spin size="large" />
        <Text type="secondary">SSO 登录中，即将跳转…</Text>
      </div>
    </ContentContainer>
  );
};

export default SsoCallback;
