import { useMemo } from 'react';
import { useUiStore } from '../store/ui';
import { useDebounced } from './useDebounced';
export function useFilters(favoritesOnly = false) {
  const sourceId = useUiStore((s) => s.sourceId);
  const group = useUiStore((s) => s.group);
  const country = useUiStore((s) => s.country);
  const rawSearch = useUiStore((s) => s.search);
  const search = useDebounced(rawSearch);
  return useMemo(() => ({ sourceId, group, country, search, favoritesOnly }), [sourceId, group, country, search, favoritesOnly]);
}
