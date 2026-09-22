/** 标签页状态 */
export interface TabState {
  name: string;
  title: string;
  path: string;
  fullPath: string;
  icon?: string;
  affix?: boolean;
  keepAlive?: boolean;
  query?: any;
}

export const useTagsViewStore = defineStore(
  "tagsView",
  () => {
    const visitedViews = ref<TabState[]>([]);
    const cachedViews = ref<string[]>([]);
    const updatedAt = ref(Date.now());
    const router = useRouter();
    const route = useRoute();

    /** 更新时间戳（轻量级变更通知，避免深度 watch） */
    function setUpdateTime() {
      updatedAt.value = Date.now();
    }

    /**
     * 添加已访问视图到已访问视图列表中
     */
    function addVisitedView(view: TabState) {
      // 如果已经存在于已访问的视图列表中或者是重定向地址，则不再添加
      const existing = visitedViews.value.find((v: TabState) => v.path === view.path);
      if (existing) {
        // 如果已存在，回填缺失的字段（如 icon）
        if (view.icon && !existing.icon) {
          existing.icon = view.icon;
        }
        return;
      }
      // 如果视图是固定的（affix），则在已访问的视图列表的开头添加
      if (view.affix) {
        visitedViews.value.unshift(view);
      } else {
        // 如果视图不是固定的，则在已访问的视图列表的末尾添加
        visitedViews.value.push(view);
      }
    }

    /**
     * 添加缓存视图到缓存视图列表中
     */
    function addCachedView({ fullPath, keepAlive }: TabState) {
      // 如果缓存视图名称已经存在于缓存视图列表中，则不再添加
      if (cachedViews.value.includes(fullPath)) {
        return;
      }

      // 如果视图需要缓存（keepAlive），则将其路由名称添加到缓存视图列表中
      if (keepAlive) {
        cachedViews.value.push(fullPath);
      }
    }

    /**
     * 从已访问视图列表中删除指定的视图
     */
    function delVisitedView(view: TabState) {
      return new Promise((resolve) => {
        for (const [i, v] of visitedViews.value.entries()) {
          // 找到与指定视图路径匹配的视图，在已访问视图列表中删除该视图
          if ((v as TabState).path === view.path) {
            visitedViews.value.splice(i, 1);
            break;
          }
        }
        resolve([...visitedViews.value]);
      });
    }

    function delCachedView(view: TabState) {
      const { fullPath } = view;
      return new Promise((resolve) => {
        const index = cachedViews.value.indexOf(fullPath);
        if (index > -1) {
          cachedViews.value.splice(index, 1);
        }
        resolve([...cachedViews.value]);
      });
    }
    function delOtherVisitedViews(view: TabState) {
      return new Promise((resolve) => {
        visitedViews.value = visitedViews.value.filter((v: TabState) => {
          return v?.affix || v.path === view.path;
        });
        resolve([...visitedViews.value]);
      });
    }

    function delOtherCachedViews(view: TabState) {
      const { fullPath } = view;
      return new Promise((resolve) => {
        const index = cachedViews.value.indexOf(fullPath);
        if (index > -1) {
          cachedViews.value = cachedViews.value.slice(index, index + 1);
        } else {
          // if index = -1, there is no cached tags
          cachedViews.value = [];
        }
        resolve([...cachedViews.value]);
      });
    }

    function updateVisitedView(view: TabState) {
      for (const v of visitedViews.value) {
        if ((v as TabState).path === view.path) {
          Object.assign(v, view);
          break;
        }
      }
    }

    /**
     * 根据路径更新标签名称
     * @param fullPath 路径
     * @param title 标签名称
     */
    function updateTagName(fullPath: string, title: string) {
      const tag = visitedViews.value.find((tag: TabState) => tag.fullPath === fullPath);

      if (tag) {
        tag.title = title;
      }
    }

    function addView(view: TabState) {
      addVisitedView(view);
      addCachedView(view);
    }

    function delView(view: TabState) {
      return new Promise((resolve) => {
        delVisitedView(view);
        delCachedView(view);
        resolve({
          visitedViews: [...visitedViews.value],
          cachedViews: [...cachedViews.value],
        });
      });
    }

    function delOtherViews(view: TabState) {
      return new Promise((resolve) => {
        delOtherVisitedViews(view);
        delOtherCachedViews(view);
        resolve({
          visitedViews: [...visitedViews.value],
          cachedViews: [...cachedViews.value],
        });
      });
    }

    function delLeftViews(view: TabState) {
      return new Promise((resolve) => {
        const currIndex = visitedViews.value.findIndex((v: TabState) => v.path === view.path);
        if (currIndex === -1) {
          return;
        }
        visitedViews.value = visitedViews.value.filter((item: TabState, index: number) => {
          if (index >= currIndex || item?.affix) {
            return true;
          }

          const cacheIndex = cachedViews.value.indexOf(item.fullPath);
          if (cacheIndex > -1) {
            cachedViews.value.splice(cacheIndex, 1);
          }
          return false;
        });
        resolve({
          visitedViews: [...visitedViews.value],
        });
      });
    }

    function delRightViews(view: TabState) {
      return new Promise((resolve) => {
        const currIndex = visitedViews.value.findIndex((v: TabState) => v.path === view.path);
        if (currIndex === -1) {
          return;
        }
        visitedViews.value = visitedViews.value.filter((item: TabState, index: number) => {
          if (index <= currIndex || item?.affix) {
            return true;
          }
          const cacheIndex = cachedViews.value.indexOf(item.fullPath);
          if (cacheIndex > -1) {
            cachedViews.value.splice(cacheIndex, 1);
          }
          return false;
        });
        resolve({
          visitedViews: [...visitedViews.value],
        });
      });
    }

    function delAllViews() {
      return new Promise((resolve) => {
        visitedViews.value = visitedViews.value.filter((tag: TabState) => tag?.affix);
        cachedViews.value = [];
        resolve({
          visitedViews: [...visitedViews.value],
          cachedViews: [...cachedViews.value],
        });
      });
    }

    function delAllVisitedViews() {
      return new Promise((resolve) => {
        visitedViews.value = visitedViews.value.filter((tag: TabState) => tag?.affix);
        resolve([...visitedViews.value]);
      });
    }

    function delAllCachedViews() {
      return new Promise((resolve) => {
        cachedViews.value = [];
        resolve([...cachedViews.value]);
      });
    }

    /**
     * 关闭当前tagView
     */
    function closeCurrentView() {
      const tags: TabState = {
        name: route.name as string,
        title: route.meta.title as string,
        path: route.path,
        fullPath: route.fullPath,
        affix: route.meta?.affix as boolean | undefined,
        keepAlive: route.meta?.keepAlive as boolean | undefined,
        query: route.query,
      };
      delView(tags).then((res: any) => {
        if (isActive(tags)) {
          toLastView(res.visitedViews, tags);
        }
      });
    }

    function isActive(tag: TabState) {
      return tag.path === route.path;
    }

    function toLastView(visitedViews: TabState[], view?: TabState) {
      const latestView = visitedViews.slice(-1)[0];
      if (latestView && latestView.fullPath) {
        router.push(latestView.fullPath);
      } else {
        // now the default is to redirect to the home page if there is no tags-view,
        // you can adjust it according to your needs.
        if (view?.name === "Dashboard") {
          // to reload home page
          router.replace(view.fullPath);
        } else {
          router.push("/");
        }
      }
    }

    /**
     * 拖拽排序：将 oldIndex 位置的标签移动到 newIndex
     */
    function sortTabs(oldIndex: number, newIndex: number) {
      const current = visitedViews.value[oldIndex];
      if (!current) return;
      visitedViews.value.splice(oldIndex, 1);
      visitedViews.value.splice(newIndex, 0, current);
      setUpdateTime();
    }

    /**
     * 切换标签固定状态，并自动重排序（固定标签在前，非固定在后）
     */
    function togglePin(view: TabState) {
      const found = visitedViews.value.find((v: TabState) => v.fullPath === view.fullPath);
      if (!found) return;

      found.affix = !found.affix;

      // 重排序：固定标签在前，非固定在后
      const affixTabs = visitedViews.value.filter((v: TabState) => v.affix);
      const normalTabs = visitedViews.value.filter((v: TabState) => !v.affix);
      visitedViews.value = [...affixTabs, ...normalTabs];

      setUpdateTime();
    }

    function $reset() {}

    /**
     * 清除非固定的标签页（用于未开启持久化时的刷新场景）
     */
    function clearNonAffixViews() {
      visitedViews.value = visitedViews.value.filter((tag: TabState) => tag?.affix);
      cachedViews.value = [];
    }

    return {
      $reset,
      clearNonAffixViews,
      visitedViews,
      cachedViews,
      updatedAt,
      addVisitedView,
      addCachedView,
      delVisitedView,
      delCachedView,
      delOtherVisitedViews,
      delOtherCachedViews,
      updateVisitedView,
      addView,
      delView,
      delOtherViews,
      delLeftViews,
      delRightViews,
      delAllViews,
      delAllVisitedViews,
      delAllCachedViews,
      closeCurrentView,
      isActive,
      toLastView,
      updateTagName,
      sortTabs,
      togglePin,
    };
  },
  {
    persist: {
      pick: ["visitedViews"],
      storage: sessionStorage,
    },
  }
);
