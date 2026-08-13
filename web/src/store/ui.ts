import { create } from "zustand";
import { persist } from "zustand/middleware";

type Theme = "system" | "light" | "dark";
type Density = "comfortable" | "compact";
type SortPreference = "relevance" | "date";

type UiState = {
  theme: Theme;
  density: Density;
  facetsOpen: boolean;
  sortPreference: SortPreference;
  setTheme: (theme: Theme) => void;
  setDensity: (density: Density) => void;
  setFacetsOpen: (open: boolean) => void;
  setSortPreference: (sortPreference: SortPreference) => void;
};

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      theme: "system",
      density: "comfortable",
      facetsOpen: false,
      sortPreference: "relevance",
      setTheme: (theme) => set({ theme }),
      setDensity: (density) => set({ density }),
      setFacetsOpen: (facetsOpen) => set({ facetsOpen }),
      setSortPreference: (sortPreference) => set({ sortPreference }),
    }),
    { name: "mailgrep-ui", partialize: ({ theme, density, sortPreference }) => ({ theme, density, sortPreference }) },
  ),
);
