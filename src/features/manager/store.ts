import { create } from "zustand";

type ViewMode = "history" | "settings";

interface ManagerStore {
  selectedItemId: string | null;
  draftText: string;
  viewMode: ViewMode;
  setSelectedItemId: (id: string | null) => void;
  setDraftText: (value: string) => void;
  setViewMode: (value: ViewMode) => void;
}

export const useManagerStore = create<ManagerStore>((set) => ({
  selectedItemId: null,
  draftText: "",
  viewMode: "history",
  setSelectedItemId: (selectedItemId) => set({ selectedItemId }),
  setDraftText: (draftText) => set({ draftText }),
  setViewMode: (viewMode) => set({ viewMode }),
}));
