export type Theme = "light" | "dark" | "system";

/**
 * Set or clear the document root's data-theme attribute. "system" clears
 * it so the existing prefers-color-scheme media query in styles.css keeps
 * driving the theme live off the OS.
 */
export function applyTheme(theme: Theme): void {
  if (theme === "system") {
    delete document.documentElement.dataset.theme;
  } else {
    document.documentElement.dataset.theme = theme;
  }
}
