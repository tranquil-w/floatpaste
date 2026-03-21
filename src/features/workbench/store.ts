import { create } from "zustand";

export type WorkbenchMode = "search" | "edit" | "empty";

export interface WorkbenchSession {
  source: "picker_edit" | "picker_search" | "global";
  initialItemId?: string;
  initialKeyword?: string;
}

interface WorkbenchStore {
  session: WorkbenchSession | null;
  setSession: (session: WorkbenchSession | null) => void;
  mode: WorkbenchMode;
  setMode: (mode: WorkbenchMode) => void;
  noticeMessage: string | null;
  setNoticeMessage: (message: string | null) => void;
  keyword: string;
  setKeyword: (keyword: string) => void;
  selectedItemId: string | null;
  setSelectedItemId: (id: string | null) => void;
  draftText: string;
  setDraftText: (text: string) => void;
  isDirty: boolean;
  setIsDirty: (dirty: boolean) => void;
  savedText: string;
  setSavedText: (savedText: string) => void;
}

export const useWorkbenchStore = create<WorkbenchStore>((set) => ({
  session: null,
  setSession: (session) => set({ session }),
  mode: "search",
  setMode: (mode) => set({ mode }),
  noticeMessage: null,
  setNoticeMessage: (noticeMessage) => set({ noticeMessage }),
  keyword: "",
  setKeyword: (keyword) => set({ keyword }),
  selectedItemId: null,
  setSelectedItemId: (selectedItemId) => set({ selectedItemId }),
  draftText: "",
  setDraftText: (draftText) => set({ draftText }),
  isDirty: false,
  setIsDirty: (isDirty) => set({ isDirty }),
  savedText: "",
  setSavedText: (savedText) => set({ savedText }),
}));
