import { create } from "zustand";

export type EditorSession = {
  itemId: string;
  source: "picker" | "search";
  returnTo: "picker" | "search";
};

type EditorStore = {
  session: EditorSession | null;
  draftText: string;
  savedText: string;
  isDirty: boolean;
  closeConfirmOpen: boolean;
  noticeMessage: string | null;
  errorMessage: string | null;
  initializeSession: (session: EditorSession) => void;
  reset: () => void;
  setDraftText: (draftText: string) => void;
  syncText: (text: string) => void;
  markSaved: (text: string) => void;
  setCloseConfirmOpen: (open: boolean) => void;
  setNoticeMessage: (message: string | null) => void;
  setErrorMessage: (message: string | null) => void;
};

const initialState = {
  session: null,
  draftText: "",
  savedText: "",
  isDirty: false,
  closeConfirmOpen: false,
  noticeMessage: null,
  errorMessage: null,
};

export const useEditorStore = create<EditorStore>((set) => ({
  ...initialState,
  initializeSession: (session) =>
    set({
      ...initialState,
      session,
    }),
  reset: () => set(initialState),
  setDraftText: (draftText) =>
    set((state) => ({
      draftText,
      isDirty: draftText !== state.savedText,
    })),
  syncText: (text) =>
    set({
      draftText: text,
      savedText: text,
      isDirty: false,
    }),
  markSaved: (text) =>
    set({
      draftText: text,
      savedText: text,
      isDirty: false,
    }),
  setCloseConfirmOpen: (closeConfirmOpen) => set({ closeConfirmOpen }),
  setNoticeMessage: (noticeMessage) => set({ noticeMessage }),
  setErrorMessage: (errorMessage) => set({ errorMessage }),
}));
