import type { App } from 'vue';

import {
  Alert,
  Button,
  Card,
  Descriptions,
  Divider,
  Drawer,
  Dropdown,
  Empty,
  Form,
  Input,
  InputNumber,
  Layout,
  Menu,
  Modal,
  Popconfirm,
  Radio,
  Select,
  Space,
  Switch,
  Table,
  Tabs,
  Tag,
  Timeline,
  Tooltip,
  Tree,
  Upload,
} from 'ant-design-vue';

/**
 * 注册全局组件
 *
 * 注意保持完整：页面模板直接使用 `<a-xxx>` kebab 标签（vben 不做 antd
 * 自动按需注册），漏注册的组件会被 Vue 当成未知元素——不报错但整块
 * 不渲染或以裸 HTML 内联泄漏（曾导致通知渠道/脚本管理/服务监控页面破版）。
 * @param app
 */
export function registerGlobComp(app: App) {
  app
    .use(Input)
    .use(Button)
    .use(Layout)
    .use(Space)
    .use(Card)
    .use(Switch)
    .use(Popconfirm)
    .use(Dropdown)
    .use(Tag)
    .use(Tabs)
    .use(Divider)
    .use(Menu)
    .use(Select)
    .use(Tree)
    .use(Form)
    .use(InputNumber)
    .use(Modal)
    .use(Tooltip)
    .use(Alert)
    .use(Descriptions)
    .use(Table)
    // Radio 的 install 同时注册 ARadio / ARadioGroup / ARadioButton
    // （AI 问数页的图表/表格切换用 a-radio-group+a-radio-button，漏注册时整块不渲染，
    // 运行时只有一条 Vue warn: Failed to resolve component: a-radio-group）
    .use(Radio)
    // Upload 的 install 同时注册 AUpload 与 AUploadDragger（Excel 导入弹窗用 a-upload-dragger）
    .use(Upload)
    // Drawer：六处弹窗改抽屉（通知渠道/规则/模板、监控告警、账号绑定、套餐配额）模板用裸 <a-drawer>
    // Empty：监控告警页空态用 <a-empty>。两者漏注册时只有 Vue warn、抽屉/空态整块不渲染
    .use(Drawer)
    .use(Empty)
    // Timeline：AI Agent 运行台账抽屉的逐节点轨迹用 <a-timeline>+<a-timeline-item>，
    // install 同时注册 ATimeline / ATimelineItem（同 Radio 模式），漏注册时轨迹整块不渲染
    .use(Timeline);
}
