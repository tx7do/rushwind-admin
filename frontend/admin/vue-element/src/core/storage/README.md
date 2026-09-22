# StorageManager 存储管理器

> 面向 admin 业务逻辑开发者的使用指南

## 简介

`StorageManager` 是对 `localStorage` / `sessionStorage` 的封装，提供 **TTL 过期、前缀隔离、批量操作、容量驱逐、多标签页同步、性能监控** 等能力。

业务层无需关心序列化、过期清理、配额溢出等底层细节。

---

## 快速开始

```ts
import { StorageManager } from "@/core/storage";

// 最简单的用法 —— 等价于直接使用 localStorage
const store = new StorageManager();

store.setItem("token", "abc123");
const token = store.getItem<string>("token"); // "abc123"
```

---

## 创建实例

```ts
const store = new StorageManager({
  prefix: "my-module",       // 键名前缀，隔离不同模块
  storageType: "localStorage", // 或 "sessionStorage"
  defaultTTL: 30 * 60 * 1000, // 默认过期时间 30 分钟
  maxItems: 200,              // 最多存储 200 条
  maxUsageMB: 5,              // 最多使用 5MB
  evictionStrategy: "hybrid", // 驱逐策略: "lru" | "lfu" | "hybrid"
});
```

| 参数 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `prefix` | `string` | `""` | 键名前缀，避免命名冲突 |
| `storageType` | `"localStorage" \| "sessionStorage"` | `"localStorage"` | 底层存储引擎 |
| `defaultTTL` | `number \| null` | `null` | 默认过期时间(ms)，`null` = 永不过期 |
| `maxItems` | `number` | `0` | 最大条目数，`0` = 不限制 |
| `maxUsageMB` | `number` | `10` | 最大使用量(MB)，`0` = 不限制 |
| `evictionStrategy` | `"lru" \| "lfu" \| "hybrid"` | `"hybrid"` | 容量超限时的驱逐策略 |
| `compressionThreshold` | `number` | `1024` | 触发压缩的阈值(bytes) |
| `enableSync` | `boolean` | `true` | 是否启用多标签页同步 |
| `onMetrics` | `(metrics) => void` | — | 性能指标回调 |
| `onSync` | `(event) => void` | — | 同步事件回调 |

---

## 核心 API

### get / set

```ts
// 存储
store.setItem("user", { name: "张三", age: 28 });

// 读取（泛型自动推断）
const user = store.getItem<{ name: string; age: number }>("user");

// 带默认值读取（键不存在时返回默认值而非 null）
const name = store.getItem<string>("name", "匿名");

// 带过期时间（单位 ms）—— 1 小时后自动过期
store.setItem("verifyCode", "123456", 60 * 60 * 1000);

// 永不过期（显式传入 null）
store.setItem("config", { theme: "dark" }, null);
```

### remove / has / clear

```ts
store.removeItem("token");          // 删除单个
store.hasItem("token");             // true / false
store.clear();                      // 清除本实例前缀下的所有项
store.clearExpiredItems();          // 只清除过期项
```

### length / key

```ts
store.length();   // 当前前缀下的有效条目数
store.key(0);     // 按索引获取键名
```

---

## 批量操作

批量 API 均返回 `Promise`，内部每 50 项让出主线程，适合大量数据场景。

### 批量获取

```ts
const values = await store.getItems<string>(["key1", "key2", "key3"]);
// { key1: "val1", key2: null, key3: "val3" }

// 带进度回调
await store.getItems<string>(keys, {}, (progress) => {
  console.log(`${progress.current}/${progress.total} - ${progress.key}: ${progress.status}`);
});
```

### 批量设置

```ts
// 简单模式 —— 所有项使用默认 TTL
const result = await store.setItems({
  key1: "value1",
  key2: "value2",
});

// 单独 TTL 模式 —— 每个项可独立设置过期时间
const result = await store.setItems({
  key1: "value1",
  key2: { value: "临时数据", ttl: 5000 },  // 5 秒后过期
  key3: { value: "永久数据", ttl: null },   // 永不过期
});

// result.success  → { key1: "value1", key2: "value2" }
// result.failed   → { key4: Error }（失败的项）
// result.expired  → []（被清理的过期键）
```

### 批量删除 / 批量检查

```ts
const { removed, failed } = await store.removeItems(["key1", "key2"]);
const exists = await store.hasItems(["key1", "key2", "key3"]);
// { key1: true, key2: false, key3: true }
```

---

## 数据格式

所有通过 `setItem` 存入的数据会自动包装为 `StorageValue` 结构：

```json
{
  "data": <实际数据>,
  "expiry": 1716900000000   // 过期时间戳，null 表示永不过期
}
```

读取时自动解包并检查过期，业务层直接拿到的是原始数据，无需手动处理。

---

## 过期机制 (TTL)

```ts
// 实例级默认 TTL
const store = new StorageManager({ defaultTTL: 60 * 1000 }); // 1 分钟

// 写入时不指定 TTL → 使用实例默认值
store.setItem("a", 1);  // 1 分钟后过期

// 写入时显式指定 TTL → 覆盖默认值
store.setItem("b", 2, 5 * 60 * 1000);  // 5 分钟后过期

// 写入时传 null → 永不过期
store.setItem("c", 3, null);
```

过期数据在 `getItem` 时自动清理，无需手动调用 `clearExpiredItems()`。

---

## 驱逐策略

当 `maxItems` 或 `maxUsageMB` 设置了限制（>0）时，每次 `setItem` 后自动检查容量。超限时按策略驱逐：

| 策略 | 说明 |
|---|---|
| `lru` | 最近最少使用 — 驱逐最久没被访问的项 |
| `lfu` | 最不经常使用 — 驱逐访问次数最少的项 |
| `hybrid` | 混合策略（默认） — 频率权重 60%，新鲜度权重 40% |

```ts
const store = new StorageManager({
  prefix: "cache",
  maxItems: 100,
  maxUsageMB: 5,
  evictionStrategy: "hybrid",
});
```

> **注意**：`maxItems=0` 且 `maxUsageMB=0` 表示不限制，不会触发驱逐。

---

## 多标签页同步

通过 `BroadcastChannel` API 实现，默认开启。

```ts
const store = new StorageManager({
  prefix: "app",
  enableSync: true,  // 默认就是 true
  onSync: (event) => {
    console.log(`标签页 ${event.tabId} 执行了 ${event.type} 操作，键: ${event.key}`);
  },
});
```

当另一个标签页执行 `setItem`、`removeItem`、`clear` 时，当前标签页会收到同步事件。

---

## 性能监控

```ts
const store = new StorageManager({
  prefix: "app",
  onMetrics: (metrics) => {
    console.log("命中率:", (metrics.hitRate * 100).toFixed(1) + "%");
    console.log("配额使用:", (metrics.quotaUsageBytes / 1024 / 1024).toFixed(2) + "MB");
  },
});

// 手动获取指标
const metrics = store.getMetrics();
// { hits, misses, expirations, sets, errors, quotaUsageBytes, hitRate }

// 重置指标
store.resetMetrics();
```

| 指标 | 说明 |
|---|---|
| `hits` | 命中次数 |
| `misses` | 未命中次数 |
| `hitRate` | 命中率 (0-1) |
| `expirations` | 过期清理次数 |
| `sets` | 设置操作次数 |
| `errors` | 错误次数 |
| `quotaUsageBytes` | 配额使用量(bytes) |

---

## 前缀隔离

不同 `prefix` 的实例互不干扰：

```ts
const userStore = new StorageManager({ prefix: "user" });
const appStore = new StorageManager({ prefix: "app" });

userStore.setItem("name", "张三");
appStore.setItem("name", "GoWind Admin");

userStore.getItem("name"); // "张三"
appStore.getItem("name");  // "GoWind Admin"

userStore.clear();  // 只清除 user 前缀的项，不影响 app
```

---

## 实际使用示例

### 偏好设置持久化

```ts
// 来自 preferences.ts 的实际用法
const cache = new StorageManager({ prefix: namespace });

// 保存
cache.setItem(STORAGE_KEY, preferencesState);

// 读取
const saved = cache.getItem<Preferences>(STORAGE_KEY);
```

### 带过期的 API 缓存

```ts
const cache = new StorageManager({ prefix: "api-cache", defaultTTL: 5 * 60 * 1000 });

async function fetchWithCache<T>(key: string, fetcher: () => Promise<T>): Promise<T> {
  const cached = cache.getItem<T>(key);
  if (cached !== null) return cached;

  const data = await fetcher();
  cache.setItem(key, data);
  return data;
}
```

### 表单草稿自动保存

```ts
const draftStore = new StorageManager({ prefix: "draft", defaultTTL: 24 * 60 * 60 * 1000 });

// 自动保存草稿（24 小时有效）
function saveDraft(formId: string, data: Record<string, any>) {
  draftStore.setItem(formId, data);
}

// 恢复草稿
function restoreDraft(formId: string) {
  return draft.getItem<Record<string, any>>(formId);
}
```

---

## 底层访问

```ts
// 获取原生 Storage 实例（仅在需要与第三方库共享时使用）
const rawStorage = store.getRawStorage();
```

---

## 导入路径

```ts
// 命名导出
import { StorageManager } from "@/core/storage";

// 类型导出（用于类型注解）
import type { StorageManagerOptions, StorageMetrics, SyncMessage } from "@/core/storage";
```
