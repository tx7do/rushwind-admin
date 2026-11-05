import { Modal, Table, Tag, Typography } from 'antd';
import type { ColumnsType } from 'antd/es/table';
import { useTranslation } from 'react-i18next';
import { useInspectSystemTasks } from '@/api/hooks/task-monitor';
import { formatDateTime } from '@/utils/date';

const { Text } = Typography;

/**
 * 系统级常驻任务弹窗（方案 C'：只读 asynq Inspector，零新表）。
 *
 * 数据来自 asynq 本身：
 *  - 调度计划表：cron + 下次/上次入队时间（上次为空 = 注册后从未运行）；
 *  - 运行状态表：active/pending/retry/archived 计数 + 最近失败明细
 *    （错误原文/失败时间/已重试次数，失败时间倒序、每类型最多 10 条）。
 * asynq 队列无命名空间：后端只汇总调度条目覆盖到的任务类型，
 * 共库错开 URI 后看到的即本实例自己的任务。
 */
const SystemTasksModal = ({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) => {
  const { t } = useTranslation('task');
  const inspectQuery = useInspectSystemTasks({ enabled: open });

  const scheduleColumns: ColumnsType<Record<string, any>> = [
    {
      title: t('sysTaskType'),
      dataIndex: 'taskType',
      render: (v: string) => <Tag color="geekblue">{v}</Tag>,
    },
    { title: t('sysTaskCron'), dataIndex: 'cronSpec', width: 130 },
    {
      title: t('sysTaskNext'),
      dataIndex: 'nextEnqueueAt',
      width: 170,
      render: (v?: string) => (v ? formatDateTime(v) : '-'),
    },
    {
      title: t('sysTaskPrev'),
      dataIndex: 'prevEnqueueAt',
      width: 170,
      render: (v?: string) =>
        v ? (
          formatDateTime(v)
        ) : (
          <Text type="secondary">{t('sysTaskNeverRun')}</Text>
        ),
    },
  ];

  const summaryColumns: ColumnsType<Record<string, any>> = [
    {
      title: t('sysTaskType'),
      dataIndex: 'taskType',
      render: (v: string) => <Tag color="geekblue">{v}</Tag>,
    },
    { title: t('sysTaskActive'), dataIndex: 'active', width: 70, align: 'center' },
    { title: t('sysTaskPending'), dataIndex: 'pending', width: 80, align: 'center' },
    {
      title: t('sysTaskRetry'),
      dataIndex: 'retry',
      width: 70,
      align: 'center',
      render: (v: number) => (v > 0 ? <Tag color="warning">{v}</Tag> : v),
    },
    {
      title: t('sysTaskArchived'),
      dataIndex: 'archived',
      width: 80,
      align: 'center',
      render: (v: number) => (v > 0 ? <Tag color="error">{v}</Tag> : v),
    },
  ];

  const failureColumns: ColumnsType<Record<string, any>> = [
    { title: t('sysTaskType'), dataIndex: 'taskType', width: 160 },
    {
      title: t('sysTaskState'),
      dataIndex: 'state',
      width: 90,
      render: (v: string) => (
        <Tag color={v === 'archived' ? 'error' : 'warning'}>{v}</Tag>
      ),
    },
    { title: t('sysTaskLastError'), dataIndex: 'lastError', ellipsis: true },
    {
      title: t('sysTaskFailedAt'),
      dataIndex: 'lastFailedAt',
      width: 170,
      render: (v?: string) => (v ? formatDateTime(v) : '-'),
    },
    {
      title: t('sysTaskRetried'),
      key: 'retried',
      width: 90,
      align: 'center',
      render: (_: unknown, r: any) => `${r.retried ?? 0}/${r.maxRetry ?? 0}`,
    },
  ];

  const summaries = inspectQuery.data?.summaries ?? [];
  const failures = summaries.flatMap((s: any) => s.recentFailures ?? []);

  return (
    <Modal
      title={t('sysTasksTitle')}
      open={open}
      onCancel={onClose}
      footer={null}
      width={860}
      destroyOnHidden
    >
      {inspectQuery.isFetching && (
        <div style={{ padding: 48, textAlign: 'center' }}>{t('sysTaskLoading')}</div>
      )}
      {inspectQuery.isError && (
        <Text type="danger">
          {inspectQuery.error?.message || t('sysTaskLoadFailed')}
        </Text>
      )}
      {inspectQuery.data && (
        <>
          <Typography.Title level={5} style={{ marginBottom: 8 }}>
            {t('sysTaskSchedules')}
            <Text type="secondary" style={{ marginLeft: 8, fontSize: 12 }}>
              {t('sysTaskQueueHint', { queue: inspectQuery.data.queue ?? '' })}
            </Text>
          </Typography.Title>
          <Table
            columns={scheduleColumns}
            dataSource={(inspectQuery.data.schedules ?? []) as Record<string, any>[]}
            pagination={false}
            rowKey="taskType"
            size="small"
            bordered
          />

          <Typography.Title level={5} style={{ margin: '20px 0 8px' }}>
            {t('sysTaskStates')}
          </Typography.Title>
          <Table
            columns={summaryColumns}
            dataSource={summaries as Record<string, any>[]}
            pagination={false}
            rowKey="taskType"
            size="small"
            bordered
          />

          <Typography.Title level={5} style={{ margin: '20px 0 8px' }}>
            {t('sysTaskFailures')}
          </Typography.Title>
          {failures.length === 0 ? (
            <Text type="secondary">{t('sysTaskNoFailures')}</Text>
          ) : (
            <Table
              columns={failureColumns}
              dataSource={failures as Record<string, any>[]}
              pagination={false}
              rowKey={(r: any) =>
                `${r.taskType}-${r.state}-${r.lastFailedAt ?? ''}-${r.retried ?? 0}`
              }
              size="small"
              bordered
            />
          )}
        </>
      )}
    </Modal>
  );
};

export default SystemTasksModal;
