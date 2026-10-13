import { useEffect, useMemo, useRef, useSyncExternalStore } from 'react';
import { api } from '../lib/bridge';
import { CataloguePager } from '../lib/catalogue-pager';
import type { ChannelFilters } from '../lib/contracts';
import { useUiStore } from '../store/ui';

export function useCatalogue(filters: ChannelFilters, initialIndex = 0) {
  const pager = useMemo(() => new CataloguePager(api.channels), []);
  const snapshot = useSyncExternalStore(pager.subscribe, pager.getSnapshot);
  const epoch = useUiStore((state) => state.catalogueEpoch);
  // The filter key the pager currently serves; null until the first reset after (re)mounting.
  const applied = useRef<string | null>(null);
  const key = JSON.stringify(filters);
  // biome-ignore lint/correctness/useExhaustiveDependencies: epoch is a refresh trigger, not a value the effect reads.
  useEffect(() => {
    // A catalogue change keeps the visible window; only a new filter starts a new result set.
    if (applied.current === key) {
      pager.refresh();
      return;
    }
    pager.reset(JSON.parse(key) as ChannelFilters, applied.current === null ? initialIndex : 0);
    applied.current = key;
  }, [pager, key, epoch, initialIndex]);
  useEffect(
    () => () => {
      applied.current = null;
      pager.dispose();
    },
    [pager],
  );
  return { pager, ...snapshot };
}
