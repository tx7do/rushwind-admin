import { useEffect, useRef, useState } from 'react';
import type { ProFormInstance } from '@ant-design/pro-components';
import {
  DrawerForm,
  ProFormText,
  ProFormDigit,
  ProFormTextArea,
  ProFormRadio,
  ProFormItem,
  ProFormCheckbox,
} from '@ant-design/pro-components';
import { App, AutoComplete } from 'antd';
import { useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import type { scriptservicev1_Script as Script } from '@/api/generated/admin/service/v1';
import { useCreateScript, useUpdateScript, useListHookPoints } from '@/api/hooks/script';
import { useDrawerCloseGuard } from '@/components/common/use-drawer-close-guard';
import { Editor, EditorType } from '@/components/common/Editor';

interface ScriptDrawerProps {
  open: boolean;
  mode: 'create' | 'edit';
  data?: Script;
  onClose: () => void;
  onSuccess: () => void;
}

const DEFAULT_LUA_SOURCE = `local log = require "kratos_logger"

function execute()
    local ctx = __get_ctx()
    -- ctx.set("key", value)
    return true
end
`;

const DEFAULT_JS_SOURCE = `function execute() {
    var ctx = __get_ctx();
    // ctx.set("key", value);
    return true;
}
`;

/** 源码是否仍为「未编辑」状态（空或恰好等于内置骨架）——切语言时才允许自动换骨架 */
const isPristineSource = (src: string) =>
  !src.trim() || src === DEFAULT_LUA_SOURCE || src === DEFAULT_JS_SOURCE;

const skeletonFor = (lang: 'LUA' | 'JAVASCRIPT') =>
  lang === 'LUA' ? DEFAULT_LUA_SOURCE : DEFAULT_JS_SOURCE;

/**
 * 脚本编辑/创建抽屉组件
 */
const ScriptDrawer: React.FC<ScriptDrawerProps> = ({ open, mode, data, onClose, onSuccess }) => {
  const { t } = useTranslation('script');
  const formRef = useRef<ProFormInstance>(null);

  // 关闭守卫：遮罩/ESC/X/取消统一，脏数据先确认（docs/form_drawer_close_guard.md）
  const { guardClose, handleValuesChange } = useDrawerCloseGuard(open, onClose);
  const queryClient = useQueryClient();
  const { message, modal } = App.useApp();

  const [confirmLoading, setConfirmLoading] = useState(false);
  const [source, setSource] = useState('');
  const [language, setLanguage] = useState<'LUA' | 'JAVASCRIPT'>('LUA');

  // 钩子点下拉（允许自由输入）
  const hookPointsQuery = useListHookPoints({ enabled: open });
  const hookPointOptions = (hookPointsQuery.data?.items || []).map((hp) => ({
    label: hp.description ? `${hp.name}（${hp.description}）` : hp.name,
    value: hp.name || '',
  }));

  // 编辑模式填充表单
  useEffect(() => {
    if (open && mode === 'edit' && data) {
      setTimeout(() => {
        formRef.current?.setFieldsValue({
          name: data.name || '',
          language: data.language || 'LUA',
          hookPoint: data.hookPoint || '',
          priority: data.priority ?? 0,
          description: data.description || '',
          critical: !!data.critical,
        });
        setSource(data.source || '');
        setLanguage((data.language || 'LUA') as 'LUA' | 'JAVASCRIPT');
      }, 0);
    }
    if (open && mode === 'create') {
      setSource(DEFAULT_LUA_SOURCE);
      setLanguage('LUA');
    }
  }, [open, mode, data]);

  const createMutation = useCreateScript({
    onSuccess: () => {
      message.success(t('createSuccess'));
      onSuccess();
      onClose();
      queryClient.invalidateQueries({ queryKey: ['listScripts'] });
    },
    onError: (error: Error) => {
      message.error(error.message || t('createFailed'));
    },
  });

  const updateMutation = useUpdateScript({
    onSuccess: () => {
      message.success(t('updateSuccess'));
      onSuccess();
      onClose();
      queryClient.invalidateQueries({ queryKey: ['listScripts'] });
    },
    onError: (error: Error) => {
      message.error(error.message || t('updateFailed'));
    },
  });

  const handleSubmit = async (values: any) => {
    if (!source.trim()) {
      message.error(t('requiredSource'));
      return;
    }
    setConfirmLoading(true);
    try {
      const payload: Record<string, any> = {
        ...values,
        source,
      };

      if (mode === 'create') {
        await createMutation.mutateAsync({ data: payload as any });
      } else if (data?.id) {
        await updateMutation.mutateAsync({ id: data.id, values: payload });
      }
    } finally {
      setConfirmLoading(false);
    }
  };

  return (
    <DrawerForm
      formRef={formRef}
      title={mode === 'create' ? t('create') : t('edit')}
      open={open}
      onValuesChange={handleValuesChange}
      initialValues={{
        language: 'LUA',
        priority: 0,
        critical: false,
        isEnabled: true,
      }}
      onFinish={handleSubmit}
      submitter={{
        searchConfig: {
          submitText: t('common:button.submit'),
          resetText: t('common:button.cancel'),
        },
        submitButtonProps: {
          loading: confirmLoading || createMutation.isPending || updateMutation.isPending,
        },
        resetButtonProps: { onClick: guardClose },
      }}
      drawerProps={{ destroyOnHidden: true, onClose: guardClose, size: 720 }}
      layout="horizontal"
      labelCol={{ flex: '0 0 120px' }}
      wrapperCol={{ flex: 'auto' }}
    >
      <ProFormText
        name="name"
        label={t('name')}
        placeholder={t('namePlaceholder')}
        rules={[
          { required: true, message: t('requiredName') },
          {
            pattern: /^[A-Za-z][A-Za-z0-9_.-]*$/,
            message: t('namePlaceholder'),
          },
        ]}
        fieldProps={{ allowClear: true }}
      />

      <ProFormRadio.Group
        name="language"
        label={t('language')}
        rules={[{ required: true }]}
        options={[
          { label: t('languageLua'), value: 'LUA' },
          { label: t('languageJavascript'), value: 'JAVASCRIPT' },
        ]}
        fieldProps={{
          optionType: 'button',
          buttonStyle: 'solid',
          onChange: (e: any) => {
            const next = e.target.value as 'LUA' | 'JAVASCRIPT';
            if (next === language) return;

            // 源码仍是骨架/为空（未编辑）：直接换新语言骨架
            if (isPristineSource(source)) {
              setLanguage(next);
              setSource(skeletonFor(next));
              return;
            }

            // 已有真实代码：不覆盖源码，但必须让用户知道「代码按新语言执行」的后果；
            // 取消则回退语言选择，避免表单值与源码静默不一致
            modal.confirm({
              title: t('switchLanguageTitle'),
              content: t('switchLanguageConfirm'),
              okText: t('common:button.ok'),
              cancelText: t('common:button.cancel'),
              onOk: () => setLanguage(next),
              onCancel: () => formRef.current?.setFieldsValue({ language }),
            });
          },
        }}
        extra={language === 'LUA' ? t('languageLuaTip') : t('languageJavascriptTip')}
      />

      <ProFormItem
        name="hookPoint"
        label={t('hookPoint')}
        extra={t('hookPointPlaceholder')}
      >
        <AutoComplete
          options={hookPointOptions}
          filterOption
          allowClear
          placeholder={t('hookPointPlaceholder')}
        />
      </ProFormItem>

      <ProFormDigit
        name="priority"
        label={t('priority')}
        placeholder={t('priorityPlaceholder')}
        fieldProps={{ precision: 0 }}
      />

      <ProFormTextArea
        name="description"
        label={t('description')}
        placeholder={t('descriptionPlaceholder')}
        fieldProps={{ allowClear: true, rows: 2 }}
      />

      <ProFormCheckbox name="critical" extra={t('criticalTip')}>
        {t('critical')}
      </ProFormCheckbox>

      {/* 源码编辑器（monaco，按语言高亮） */}
      <div className="mb-6">
        <label className="mb-2 block text-sm font-medium text-[color:var(--ant-color-text)]">
          <span className="text-[color:var(--ant-color-error)]">* </span>
          {t('source')}
        </label>
        <div className="rounded-lg border border-[color:var(--ant-color-border-secondary)]">
          <Editor
            key={language}
            editorType={EditorType.CODE}
            height={360}
            value={source}
            codeOptions={{
              language: language === 'LUA' ? 'lua' : 'javascript',
              tabSize: 4,
              // 关闭 hljs 自动探测：其语言表缺 lua，会把 Lua 骨架误判为 plaintext 失去高亮
              autoDetectLanguage: false,
            }}
            onChange={(v) => setSource(v ?? '')}
          />
        </div>
      </div>
    </DrawerForm>
  );
};

export default ScriptDrawer;
