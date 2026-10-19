// Applies the theme setting. "system" removes the attribute so the
// prefers-color-scheme rules in the stylesheets decide; "dark" and "light" force it.
export function applyTheme(theme) {
  const root = document.documentElement;
  if (theme === "dark" || theme === "light") root.dataset.theme = theme;
  else delete root.dataset.theme;
}
