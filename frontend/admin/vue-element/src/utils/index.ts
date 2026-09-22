/**
 * 工具函数统一导出
 */

// 数据验证
export { isExternal, isValidURL, isEmail, isMobile, VALIDATORS } from "./validate";

// 文件下载
export { downloadFile } from "./download";

export * from "./crypto";
export * from "./color";
export * from "./format";
export * from "./theme";
export * from "./utils";
export * from "./query";
export * from "./merge";
export * from "./window";
export * from "./nprogress";
export * from "./tree";
export * from "./inference";
export * from "./date";
export * from "./letter";
export * from "./dom";
export * from "./state-handler";
export * from "./cn";

export { default as cloneDeep } from "lodash.clonedeep";
