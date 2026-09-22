type LoadMessageFn = (lang: string) => Promise<Record<string, string> | undefined>;

interface LocaleSetupOptions {
  /**
   * Default language
   * @default zh-CN
   */
  defaultLocale?: string;
  /**
   * Load message function
   * @param lang
   * @returns
   */
  loadMessages?: LoadMessageFn;
  /**
   * Whether to warn when the key is not found
   */
  missingWarn?: boolean;
}

type ImportLocaleFn = () => Promise<{ default: Record<string, string> }>;

export type { LoadMessageFn, LocaleSetupOptions, ImportLocaleFn };
