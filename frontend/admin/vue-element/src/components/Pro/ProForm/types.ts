import type { ColProps, FormItemRule } from "element-plus";

/** 表单组件类型枚举 */
export type FormValueType =
  | "input"
  | "textarea"
  | "select"
  | "radio"
  | "checkbox"
  | "switch"
  | "date-picker"
  | "time-picker"
  | "time-select"
  | "input-number"
  | "cascader"
  | "tree-select"
  | "api-tree-select"
  | "input-tag"
  | "custom-tag"
  | "icon-select"
  | "number"
  | "date"
  | "custom";

/** 字段Key类型 */
export type FieldKey<T> = string & keyof T;

/** Pro 表单字段配置，被 ProForm、ProSearch、ProModal 共用 */
export interface ProFormField<T = Record<string, any>> {
  /** 组件类型，默认 input */
  type?: FormValueType;
  /** 标签文本 */
  label: string;
  /** 字段名 */
  field: FieldKey<T>;
  /** 标签提示 */
  tips?: string | Record<string, any>;
  /** 组件属性 */
  attrs?: Record<string, any>;
  /** 选择项 */
  options?: { label: string; value: any; disabled?: boolean; [key: string]: any }[];
  /** 校验规则 */
  rules?: FormItemRule[];
  /** 初始值 */
  initialValue?: any;
  /** 自定义插槽名 */
  slotName?: string;
  /** 是否隐藏 */
  hidden?: boolean;
  /** 显隐联动函数 */
  displayIf?: (model: T) => boolean;
  /** 栅格占位 */
  span?: number;
  /** ElCol 配置 */
  col?: Partial<ColProps>;
  /** 组件事件 */
  events?: Record<string, (...args: any[]) => void>;
  /** 初始化函数 */
  initFn?: (field: ProFormField<T>) => void;
  /** 异步数据源 */
  api?: () => Promise<any[]>;
}
