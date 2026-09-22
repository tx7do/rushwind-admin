import type { Recordable } from '@vben/types';
import type { VxeGridProps, VxeUIExport } from 'vxe-table';

import type { VxeGridApi } from './api';

import { formatDate, formatDateTime, isFunction } from '@vben/utils';

export function extendProxyOptions(
  api: VxeGridApi,
  options: VxeGridProps,
  getFormValues: () => Recordable<any>,
) {
  [
    'query',
    'querySuccess',
    'queryError',
    'queryAll',
    'queryAllSuccess',
    'queryAllError',
  ].forEach((key) => {
    extendProxyOption(key, api, options, getFormValues);
  });
}

function extendProxyOption(
  key: string,
  api: VxeGridApi,
  options: VxeGridProps,
  getFormValues: () => Recordable<any>,
) {
  const { proxyConfig } = options;
  const configFn = (proxyConfig?.ajax as Recordable<any>)?.[key];
  if (!isFunction(configFn)) {
    return options;
  }

  const wrapperFn = async (
    params: Recordable<any>,
    customValues: Recordable<any>,
    ...args: Recordable<any>[]
  ) => {
    const formValues = getFormValues();
    const data = await configFn(
      params,
      {
        /**
         * 开启toolbarConfig.refresh功能
         * 点击刷新按钮 这里的值为PointerEvent 会携带错误参数
         */
        ...(customValues instanceof PointerEvent ? {} : customValues),
        ...formValues,
      },
      ...args,
    );
    return data;
  };
  api.setState({
    gridOptions: {
      proxyConfig: {
        ajax: {
          [key]: wrapperFn,
        },
      },
    },
  });
}

export function extendsDefaultFormatter(vxeUI: VxeUIExport) {
  vxeUI.formats.add('formatDate', {
    tableCellFormatMethod({ cellValue }) {
      return formatDate(cellValue);
    },
  });

  vxeUI.formats.add('formatDateTime', {
    tableCellFormatMethod({ cellValue }) {
      return formatDateTime(cellValue);
    },
  });

  vxeUI.formats.add('formatCent', {
    tableCellFormatMethod({ cellValue }) {
      if (
        cellValue === null ||
        cellValue === undefined ||
        Number.isNaN(cellValue)
      ) {
        return '-';
      }
      // 假设金额以分为单位，转换为元并保留两位小数
      return (cellValue / 100).toLocaleString(undefined, {
        minimumFractionDigits: 2,
        maximumFractionDigits: 2,
      });
    },
  });

  vxeUI.formats.add('formatPercent', {
    tableCellFormatMethod({ cellValue }) {
      if (
        cellValue === null ||
        cellValue === undefined ||
        Number.isNaN(cellValue)
      ) {
        return '-';
      }
      return `${(Number(cellValue) * 100).toFixed(2).replace(/\.00$/, '')}%`;
    },
  });

  vxeUI.formats.add('formatThousand', {
    tableCellFormatMethod({ cellValue }) {
      if (
        cellValue === null ||
        cellValue === undefined ||
        Number.isNaN(cellValue)
      ) {
        return '-';
      }
      return Number(cellValue).toLocaleString();
    },
  });

  vxeUI.formats.add('formatStr', {
    tableCellFormatMethod({ cellValue }) {
      const str = String(cellValue);
      // 如果是数字，前面补零
      if (/^\d+$/.test(str)) {
        return str.padStart(2, '0');
      }
      return str;
    },
  });
}
