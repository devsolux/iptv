import { useEffect, useMemo, useRef, useSyncExternalStore } from 'react';
import { api } from '../lib/bridge';
import { CataloguePager } from '../lib/catalogue-pager';
import type { ChannelFilters } from '../lib/contracts';
import { useUiStore } from '../store/ui';

export function useCatalogue(filters: ChannelFilters, initialIndex = 0) {
  const pager = useMemo(() => new CataloguePager(api.channels), []);
  const snapshot = useSyncExternalStore(pager.subscribe, pager.getSnapshot);
  const epoch = useUiStore((state) => state.catalogueEpoch);
  const first = useRef(true);
  const key = JSON.stringify(filters);
  useEffect(() => {
    const parsed = JSON.parse(key) as ChannelFilters;
    pager.reset(parsed, first.current ? initialIndex : 0);
    first.current = false;
  }, [pager, key, epoch, initialIndex]);
  useEffect(() => () => pager.dispose(), [pager]);
  return { pager, ...snapshot };
}
