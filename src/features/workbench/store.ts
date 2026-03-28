import { create } from "zustand";

export interface WorkbenchSession {
  source: "global";
  initialItemId?: string;
  initialKeyword?: string;
}

type WorkbenchStore = {
  session: WorkbenchSession | null;
  noticeMessage: string | null;
  keyword: string;
  selectedItemId: string | null;
  setSession: (session: WorkbenchSession | null) => void;
  setNoticeMessage: (message: string | null) => void;
  setKeyword: (keyword: string) => void;
  setSelectedItemId: (id: string | null) => void;
  reset: () => void;
};

const initialState = {
  session: null,
  noticeMessage: null,
  keyword: "",
  selectedItemId: null,
};

export const useWorkbenchStore = create<WorkbenchStore>((set) => ({
  ...initialState,
  setSession: (session) => set({ session }),
  setNoticeMessage: (noticeMessage) => set({ noticeMessage }),
  setKeyword: (keyword) => set({ keyword }),
  setSelectedItemId: (selectedItemId) => set({ selectedItemId }),
  reset: () => set(initialState),
}));
