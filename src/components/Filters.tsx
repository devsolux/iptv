import { useEffect, useState } from 'react';
import { dpadScope } from '../hooks/useDpadScope';
import { api, errorMessage } from '../lib/bridge';
import type { Source } from '../lib/contracts';
import { useUiStore } from '../store/ui';
import { countryName } from './Flag';
import { Icon } from './Icon';
export function Filters({ onEnterContent }: { onEnterContent: () => void }) {
  const search = useUiStore((s) => s.search);
  const sourceId = useUiStore((s) => s.sourceId);
  const group = useUiStore((s) => s.group);
  const country = useUiStore((s) => s.country);
  const epoch = useUiStore((s) => s.catalogueEpoch);
  const [sources, setSources] = useState<Source[]>([]);
  const [groups, setGroups] = useState<string[]>([]);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    void Promise.all([api.sources(), api.groups(sourceId)])
      .then(([sources, groups]) => {
        if (active) {
          setSources(sources);
          setGroups(groups);
          setError('');
        }
      })
      .catch((error: unknown) => {
        if (active) setError(errorMessage(error));
      });
    return () => {
      active = false;
    };
  }, [sourceId, epoch]);
  return (
    <div
      className="filter-panel"
      onKeyDown={(event) => {
        if (event.key === 'Enter' && event.target instanceof HTMLInputElement) {
          event.preventDefault();
          onEnterContent();
          return;
        }
        dpadScope(event);
        if (
          !event.defaultPrevented &&
          event.key === 'ArrowRight' &&
          !(event.target instanceof HTMLInputElement)
        ) {
          event.preventDefault();
          onEnterContent();
        }
        if (
          !event.defaultPrevented &&
          event.key === 'ArrowUp' &&
          event.target instanceof HTMLInputElement
        ) {
          document.getElementById('page-actions')?.querySelector<HTMLElement>('button')?.focus();
          event.preventDefault();
        }
      }}
    >
      <div className="filters">
        <label className="search-field">
          <Icon name="search" />
          <span className="sr-only">Search channels</span>
          <input
            id="page-focus-start"
            data-focus
            type="search"
            maxLength={256}
            placeholder="Search your channels"
            value={search}
            onChange={(e) => useUiStore.getState().setSearch(e.target.value)}
            autoComplete="off"
            spellCheck={false}
          />
        </label>
        <label className="select-field">
          <span>Source</span>
          <select
            data-focus
            aria-label="Source"
            value={sourceId ?? ''}
            onChange={(e) => useUiStore.getState().setSource(e.target.value || null)}
          >
            <option value="">All sources</option>
            {sources.map((source) => (
              <option key={source.id} value={source.id}>
                {source.name}
              </option>
            ))}
          </select>
        </label>
        <label className="select-field">
          <span>Group</span>
          <select
            data-focus
            aria-label="Group"
            value={group ?? ''}
            onChange={(e) => useUiStore.getState().setGroup(e.target.value || null)}
          >
            <option value="">All groups</option>
            {groups.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
          </select>
        </label>
        {country && (
          <button
            type="button"
            data-focus
            className="filter-chip"
            onClick={() => useUiStore.getState().setCountry(null)}
          >
            {countryName(country)} <Icon name="close" size={15} />
          </button>
        )}
      </div>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
