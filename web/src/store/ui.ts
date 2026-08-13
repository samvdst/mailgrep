import { create } from "zustand";
import { persist } from "zustand/middleware";

type Theme = "system" | "light" | "dark";
type Density = "comfortable" | "compact";

type UiState = {
  theme: Theme;
  density: Density;
  facetsOpen: boolean;
  setTheme: (theme: Theme) => void;
  setDensity: (density: Density) => void;
  setFacetsOpen: (open: boolean) => void;
};

export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      theme: "system",
      density: "comfortable",
      facetsOpen: false,
      setTheme: (theme) => set({ theme }),
      setDensity: (density) => set({ density }),
      setFacetsOpen: (facetsOpen) => set({ facetsOpen }),
    }),
    { name: "mailgrep-ui", partialize: ({ theme, density }) => ({ theme, density }) },
  ),
);
