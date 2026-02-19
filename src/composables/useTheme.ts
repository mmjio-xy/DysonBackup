import { computed, ref } from "vue";
import { darkTheme, lightTheme } from "naive-ui";

export type ThemeMode = "dark" | "light";

const mode = ref<ThemeMode>("dark");

export function useTheme() {
  const naiveTheme = computed(() => (mode.value === "dark" ? darkTheme : lightTheme));
  const isDark = computed(() => mode.value === "dark");

  function toggle() {
    mode.value = mode.value === "dark" ? "light" : "dark";
  }

  return { mode, naiveTheme, isDark, toggle };
}
