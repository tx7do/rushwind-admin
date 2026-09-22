import { defineComponent, h, onBeforeUnmount } from "vue";
import type { ProPageConfig } from "../ProPage/types";
import { ProPageApi } from "./ProPageApi";
import ProPage from "../ProPage/index.vue";

/**
 * useProPage —— Level 2：命令式 Api 控制模式
 *
 * 返回 [ProPageComponent, api]，通过 api 命令式控制 ProPage 实例。
 * api.setState() 支持 mount 前预设配置、mount 后响应式更新。
 *
 * @example
 * ```ts
 * const [Page, pageApi] = useProPage(pageConfig);
 *
 * // 命令式操作
 * pageApi.refresh();
 * pageApi.openAdd();
 * pageApi.reload({ status: 1 });
 * pageApi.setState({ loading: true }); // 响应式状态更新
 * ```
 *
 * @param config - ProPage 配置对象
 * @returns [ProPage 组件, ProPageApi 实例]
 */
export function useProPage<T = any, Q = any>(config: ProPageConfig<T, Q>) {
  const api = new ProPageApi(config);

  // 创建包装组件，自动注入 api 并处理生命周期
  const Page = defineComponent(
    (props: { config?: ProPageConfig<T, Q> }, { attrs, slots }) => {
      onBeforeUnmount(() => {
        api.unmount();
      });

      // 支持运行时覆盖配置（通过 props 或 setState）
      const mergedConfig = props.config ?? api.store.config ?? config;

      return () =>
        h(
          ProPage as any,
          {
            ...attrs,
            config: mergedConfig,
            ref: (el: any) => {
              if (el) {
                api.mount(el as any);
              }
            },
          } as any,
          slots as any
        ) as any;
    },
    {
      name: "ProPageWrapper",
      inheritAttrs: false,
    }
  );

  return [Page, api] as const;
}

export type UseProPage = typeof useProPage;
