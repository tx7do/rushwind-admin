export type { LoadMessageFn, LocaleSetupOptions, ImportLocaleFn } from "./types";

export {
  $t,
  $te,
  i18n,
  loadLocaleMessages,
  loadLocalesMap,
  loadLocalesMapFromDir,
  setupI18n,
  translateRouteTitle,
  useI18n,
} from "./setup";

export type { Locale } from "vue-i18n";
