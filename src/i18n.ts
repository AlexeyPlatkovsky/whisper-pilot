// i18n scaffolding: English is the only shipped locale in beta (F005-R8).
// Adding a release locale later means adding a new key to STRINGS with the
// same shape as `en`, not restructuring this module or its call sites.

const en = {
  addFile: "Add file",
  save: "Save",
  transcribingPrefix: "Transcribing",
  emptyState: "Add an audio or video file to get a transcript.",
  modelMissing:
    "The Whisper model isn't downloaded. Open Settings → AI models to download it.",
} as const;

export type Locale = "en";
export type StringKey = keyof typeof en;

const STRINGS: Record<Locale, Record<StringKey, string>> = { en };

export function t(key: StringKey, locale: Locale = "en"): string {
  return STRINGS[locale][key];
}
