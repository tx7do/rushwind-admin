import type { Component } from "vue";
import { computed, defineComponent, h, inject, provide, reactive, toRaw, watch } from "vue";
import type { Ref } from "vue";
import { bindMethods, isFunction, mergeWithArrayOverride } from "@/utils";

// ==================== ModalApi ====================

export interface ProModalState {
  isOpen: boolean;
  confirmLoading: boolean;
  title: string;
  /** 共享数据（传递给 connectedComponent） */
  sharedData: Record<string, any>;
}

function getDefaultModalState(): ProModalState {
  return {
    isOpen: false,
    confirmLoading: false,
    title: "",
    sharedData: {},
  };
}

export class ProModalApi {
  private onBeforeCloseCallback?: () => boolean | void;
  private onCloseCallback?: () => void;
  private onConfirmCallback?: () => Promise<void>;
  private onOpenChangeCallback?: (isOpen: boolean) => void;
  private onOpenedCallback?: () => void;
  private onClosedCallback?: () => void;

  /** 响应式状态 */
  public store: ProModalState;

  constructor(options?: {
    onBeforeClose?: () => boolean | void;
    onConfirm?: () => Promise<void>;
    onClose?: () => void;
    onOpenChange?: (isOpen: boolean) => void;
    onOpened?: () => void;
    onClosed?: () => void;
  }) {
    this.onBeforeCloseCallback = options?.onBeforeClose;
    this.onConfirmCallback = options?.onConfirm;
    this.onCloseCallback = options?.onClose;
    this.onOpenChangeCallback = options?.onOpenChange;
    this.onOpenedCallback = options?.onOpened;
    this.onClosedCallback = options?.onClosed;

    this.store = reactive(getDefaultModalState()) as ProModalState;

    // 监听 isOpen 变化触发 onOpenChange
    watch(
      () => this.store.isOpen,
      (newVal, oldVal) => {
        if (newVal !== oldVal) {
          this.onOpenChangeCallback?.(newVal);
        }
      }
    );

    bindMethods(this);
  }

  /** 打开弹窗，可传递共享数据 */
  open(data?: Record<string, any>) {
    if (data) {
      this.setState((prev) => ({
        ...prev,
        sharedData: { ...prev.sharedData, ...data },
        isOpen: true,
      }));
    } else {
      this.store.isOpen = true;
    }
  }

  /** 关闭弹窗（支持 onBeforeClose 阻止关闭） */
  close() {
    const allowClose = this.onBeforeCloseCallback?.() ?? true;
    if (allowClose) {
      this.store.isOpen = false;
    }
  }

  /** 获取共享数据 */
  getData<T extends Record<string, any> = Record<string, any>>(): T {
    return (toRaw(this.store).sharedData ?? {}) as T;
  }

  /** 设置共享数据 */
  setData(data: Record<string, any>) {
    Object.assign(this.store.sharedData, data);
  }

  /** 设置确认按钮加载状态 */
  setLoading(loading: boolean) {
    this.store.confirmLoading = loading;
  }

  /** 设置标题 */
  setTitle(title: string) {
    this.store.title = title;
  }

  /** 响应式更新状态 */
  setState(stateOrFn: ((prev: ProModalState) => Partial<ProModalState>) | Partial<ProModalState>) {
    const prev = toRaw(this.store) as ProModalState;
    const update = typeof stateOrFn === "function" ? stateOrFn(prev) : stateOrFn;
    const merged = mergeWithArrayOverride(update, prev) as ProModalState;
    Object.assign(this.store, merged);
  }

  /** 确认操作 */
  async onConfirm() {
    await this.onConfirmCallback?.();
  }

  /** 取消/关闭操作 */
  onCancel() {
    if (this.onCloseCallback) {
      this.onCloseCallback();
    } else {
      this.close();
    }
  }

  /** 弹窗打开动画完毕后的回调 */
  onOpened() {
    if (this.store.isOpen) {
      this.onOpenedCallback?.();
    }
  }

  /** 弹窗关闭动画完毕后的回调 */
  onClosed() {
    if (!this.store.isOpen) {
      this.onClosedCallback?.();
    }
  }

  /** 批量更新状态 */
  batchStore(cb: () => void) {
    cb();
  }

  /** 获取状态的响应式引用 */
  useStore<T = ProModalState>(selector?: (state: NoInfer<ProModalState>) => T): Readonly<Ref<T>> {
    const sel = selector ?? ((s: any) => s);
    return computed(() => sel(this.store)) as Readonly<Ref<T>>;
  }

  /** 重置共享数据 */
  resetData() {
    this.store.sharedData = {};
  }
}

// ==================== useProModal ====================

const PRO_MODAL_INJECT_KEY = Symbol("PRO_MODAL_INJECT");

export interface UseProModalOptions {
  /** 连接的外部组件（弹窗内容组件） */
  connectedComponent?: Component;
  /** 关闭前回调，返回 false 可阻止关闭 */
  onBeforeClose?: () => boolean | void;
  /** 确认回调 */
  onConfirm?: () => Promise<void>;
  /** 关闭回调 */
  onClose?: () => void;
  /** 弹窗打开变化回调 */
  onOpenChange?: (isOpen: boolean) => void;
  /** 弹窗打开动画完毕回调 */
  onOpened?: () => void;
  /** 弹窗关闭动画完毕回调 */
  onClosed?: () => void;
}

/**
 * useProModal —— 弹窗命令式控制 + connectedComponent 抽离
 *
 * @example
 * ```ts
 * // 列表页：使用 connectedComponent 连接弹窗组件
 * const [Modal, modalApi] = useProModal({
 *   connectedComponent: UserDrawer,
 *   onOpenChange(isOpen) {
 *     if (!isOpen) pageApi.reload();
 *   },
 * });
 *
 * function handleAdd() {
 *   modalApi.open({ create: true });
 * }
 * function handleEdit(row: any) {
 *   modalApi.open({ create: false, row });
 * }
 * ```
 *
 * ```vue
 * <template>
 *   <Page />
 *   <Modal />
 * </template>
 * ```
 *
 * @example
 * ```ts
 * // 弹窗组件内部：通过 inject 获取 api
 * const modalApi = injectProModalApi();
 * const data = modalApi.getData();
 * ```
 */
export function useProModal(options: UseProModalOptions = {}) {
  const { connectedComponent } = options;

  // === connectedComponent 模式：provide/inject 桥接 ===
  if (connectedComponent) {
    const extendedApi = reactive({}) as ProModalApi;

    const Modal = defineComponent(
      (props: Record<string, any>, { attrs, slots }) => {
        provide(PRO_MODAL_INJECT_KEY, {
          extendApi(api: ProModalApi) {
            Object.setPrototypeOf(extendedApi, api);
          },
          options,
        });
        return () => h(connectedComponent, { ...props, ...attrs }, slots);
      },
      { name: "ProModalWrapper", inheritAttrs: false }
    );

    return [Modal, extendedApi] as const;
  }

  // === 非 connectedComponent 模式：直接创建 Api ===
  const api = new ProModalApi({
    onBeforeClose: options.onBeforeClose,
    onConfirm: options.onConfirm,
    onClose: options.onClose,
    onOpenChange: options.onOpenChange,
    onOpened: options.onOpened,
    onClosed: options.onClosed,
  });

  return [null as null, api] as const;
}

/**
 * 在 connectedComponent 内部注入 ProModalApi
 *
 * @example
 * ```ts
 * // UserDrawer.vue（弹窗内容组件）
 * const modalApi = injectProModalApi();
 * const data = modalApi.getData();
 * ```
 */
export function injectProModalApi(): ProModalApi {
  const injectData = inject<any>(PRO_MODAL_INJECT_KEY, {});

  // 创建内部 ModalApi
  const api = new ProModalApi({
    onBeforeClose: injectData.options?.onBeforeClose,
    onConfirm: injectData.options?.onConfirm,
    onClose: injectData.options?.onClose,
    onOpenChange: injectData.options?.onOpenChange,
    onOpened: injectData.options?.onOpened,
    onClosed: injectData.options?.onClosed,
  });

  // 连接到外部 extendedApi
  injectData.extendApi?.(api);

  return api;
}

export type UseProModal = typeof useProModal;
