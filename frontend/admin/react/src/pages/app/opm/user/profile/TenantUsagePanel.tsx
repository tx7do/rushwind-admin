import { Progress, Spin, Typography } from 'antd';
import { useTranslation } from 'react-i18next';

const { Text } = Typography;

/** 租户套餐用量面板：用户数/存储占用 vs 配额上限的进度条。 */
function TenantUsagePanel({ usageQuery }: {
  usageQuery: { data?: import('@/api/generated/admin/service/v1').identityservicev1_TenantUsage | undefined; isLoading: boolean };
}) {
  const { t } = useTranslation('profile');
  const usage = usageQuery.data;

  if (usageQuery.isLoading) {
    return <Spin style={{ display: 'block', margin: '48px auto' }} />;
  }
  if (!usage || !usage.quotas || usage.quotas.length === 0) {
    return <Text type="secondary">{t('tenantUsage.empty')}</Text>;
  }

  // 64-bit 计数经 protojson 是字符串（文档已知坑），必须 Number() 转换再运算
  const num = (v: number | string | undefined): number => Number(v ?? 0);
  const currentOf = (quotaType: string): number => {
    switch (quotaType) {
      case 'USER_LIMIT': return num(usage.userCount);
      case 'STORAGE': return Math.round(num(usage.storageUsedBytes) / 1024 / 1024);
      case 'API_CALL': return num(usage.apiCallCount);
      default: return 0;
    }
  };
  const unitOf = (quotaType: string): string =>
    quotaType === 'STORAGE' ? 'MB' : '';

  return (
    <div style={{ maxWidth: 560 }}>
      <p style={{ marginBottom: 16 }}>
        <Text type="secondary">
          {t('tenantUsage.plan')}
          {usage.planName ? <strong>{usage.planName}</strong> : <strong>{t('tenantUsage.noPlan')}</strong>}
        </Text>
      </p>
      {usage.quotas.map((q, idx) => {
        const quotaType = q.quotaType?.toString() ?? '';
        const limit = num(q.quotaValue);
        const current = currentOf(quotaType);
        const pct = limit > 0 ? Math.min(100, (current / limit) * 100) : 0;
        const unit = unitOf(quotaType);
        return (
          <div key={idx} style={{ marginBottom: 20 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 4 }}>
              <span>{t(`tenantUsage.quota.${quotaType}`, quotaType)}</span>
              <span>
                {current}{unit} / {limit}{unit}
                {pct >= 100 && (
                  <Text type="danger" style={{ marginLeft: 8 }}>{t('tenantUsage.reached')}</Text>
                )}
              </span>
            </div>
            <Progress
              percent={pct}
              size="small"
              status={pct >= 100 ? 'exception' : pct >= 80 ? 'active' : 'normal'}
            />
          </div>
        );
      })}
    </div>
  );
}

export default TenantUsagePanel;
