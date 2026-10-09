/**
 * Nexusor ships a single dark theme, so the theme is a constant rather than a
 * preference. `applyTheme` still exists as the one place that touches the DOM,
 * which keeps `JsonEditor`'s `data-theme` observer working if a second theme is
 * ever added.
 */
export const THEME_ID = "default-dark";

export function applyTheme() {
  document.documentElement.dataset.theme = THEME_ID;
}