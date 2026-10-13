import { create } from 'zustand';
import type { AppStatus } from '../lib/contracts';
interface FocusBookmark { id: string; index: number }
interface UiState {
  status: AppStatus | null;
  sourceId: string | null; group: string | null; country: string | null; search: string;
  catalogueEpoch: number; focus: Record<string, FocusBookmark | undefined>;
  setStatus: (status: AppStatus) => void;
  setSource: (sourceId: string | null) => void;
  setCountry: (country: string | null) => void;
  setGroup: (group: string | null) => void;
  setSearch: (search: string) => void;
  invalidate: () => void;
  rememberFocus: (scope: string, value: FocusBookmark) => void;
}
export const useUiStore = create<UiState>((set) => ({
  status: null, sourceId: null, group: null, country: null, search: '', catalogueEpoch: 0, focus: {},
  setStatus: (status) => set({ status }),
  setSource: (sourceId) => set({ sourceId, group: null }),
  setCountry: (country) => set({ country }),
  setGroup: (group) => set({ group }), setSearch: (search) => set({ search }),
  invalidate: () => set((state) => ({ catalogueEpoch: state.catalogueEpoch + 1 })),
  rememberFocus: (scope, value) => set((state) => {
    const focus = { ...state.focus, [scope]: value };
    while (Object.keys(focus).length > 16) {
      const oldest = Object.keys(focus).find((key) => key !== scope);
      if (!oldest) break;
      delete focus[oldest];
    }
    return { focus };
  }),
}));
