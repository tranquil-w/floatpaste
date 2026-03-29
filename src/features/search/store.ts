import { create } from "zustand";

export interface SearchSession {
  source: "global";
  initialItemId?: string;
  initialKeyword?: string;
}

type SearchStore = {
  session: SearchSession | null;
  noticeMessage: string | null;
  keyword: string;
  selectedItemId: string | null;
  setSession: (session: SearchSession | null) => void;
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

export const useSearchStore = create<SearchStore>((set) => ({
  ...initialState,
  setSession: (session) => set({ session }),
  setNoticeMessage: (noticeMessage) => set({ noticeMessage }),
  setKeyword: (keyword) => set({ keyword }),
  setSelectedItemId: (selectedItemId) => set({ selectedItemId }),
  reset: () => set(initialState),
}));
