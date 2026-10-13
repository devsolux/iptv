import { useVirtualizer } from '@tanstack/react-virtual';
import { type CSSProperties, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useNavigate } from 'react-router';
import { Empty, ErrorNotice, Loading } from '../components/Feedback';
import { countryName, Flag } from '../components/Flag';
import { Icon } from '../components/Icon';
import { focusNavigation, focusPageStart } from '../components/Shell';
import { useColumns } from '../hooks/useColumns';
import { useDpadFocus } from '../hooks/useDpadFocus';
import { api, errorMessage } from '../lib/bridge';
import { useUiStore } from '../store/ui';
export default function CountriesPage() {
  const sourceId = useUiStore((s) => s.sourceId);
  const epoch = useUiStore((s) => s.catalogueEpoch);
  const [countries, setCountries] = useState<Awaited<ReturnType<typeof api.countries>>>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [search, setSearch] = useState('');
  const navigate = useNavigate();
  const viewport = useRef<HTMLDivElement>(null);
  const columns = useColumns(viewport, 220);
  const visible = useMemo(
    () =>
      countries.filter(
        (country) =>
          countryName(country.code).toLowerCase().includes(search.toLowerCase()) ||
          country.code.toLowerCase().includes(search.toLowerCase()),
      ),
    [countries, search],
  );
  const virtual = useVirtualizer({
    count: Math.ceil(visible.length / columns),
    getScrollElement: () => viewport.current,
    estimateSize: () => 190,
    overscan: 2,
  });
  const focus = useDpadFocus({
    container: viewport,
    count: visible.length,
    columns,
    scrollTo: (index) => virtual.scrollToIndex(Math.floor(index / columns)),
    request: () => undefined,
    updateToken: visible,
  });
  useEffect(() => {
    let active = true;
    setLoading(true);
    void api
      .countries(sourceId)
      .then((data) => {
        if (active) {
          setCountries(data);
          setError('');
        }
      })
      .catch((error: unknown) => {
        if (active) setError(errorMessage(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [sourceId, epoch]);
  function open(code: string) {
    useUiStore.getState().setCountry(code);
    useUiStore.getState().setSearch('');
    navigate('/channels');
  }
  return (
    <section className="page catalogue-page">
      <header className="page-heading">
        <div>
          <span className="eyebrow">A WORLD OF CHANNELS</span>
          <h1>Countries</h1>
          <p className="muted">Explore the countries represented in your sources.</p>
        </div>
      </header>
      <label className="search-field country-search">
        <Icon name="search" />
        <span className="sr-only">Search countries</span>
        <input
          id="page-focus-start"
          type="search"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Find a country"
          onKeyDown={(event) => {
            if (event.key === 'ArrowDown') {
              event.preventDefault();
              focus.focusIndex(0);
            }
          }}
        />
      </label>
      {error && <ErrorNotice message={error} />}
      {loading && <Loading />}
      {!loading && !visible.length && (
        <Empty
          title={countries.length ? 'No matching countries.' : 'Add a country to your source.'}
        >
          <p>
            Countries come from M3U country tags or the default country configured with a source.
          </p>
          <Link className="button primary" to="/sources">
            Manage sources
          </Link>
        </Empty>
      )}
      <div
        ref={viewport}
        className="virtual-viewport"
        tabIndex={0}
        aria-label="Countries"
        onKeyDown={(event) => {
          focus.onKeyDown(event);
          if (!event.defaultPrevented && event.key === 'ArrowLeft') {
            event.preventDefault();
            focusNavigation();
          }
          if (!event.defaultPrevented && event.key === 'ArrowUp') {
            event.preventDefault();
            focusPageStart();
          }
        }}
      >
        <div className="virtual-canvas" style={{ height: virtual.getTotalSize() }}>
          {virtual.getVirtualItems().map((row) => (
            <div
              key={row.key}
              className="channel-row"
              style={
                {
                  transform: `translateY(${row.start}px)`,
                  height: row.size,
                  '--columns': columns,
                } as CSSProperties
              }
            >
              {Array.from({ length: columns }, (_, col) => row.index * columns + col).map(
                (index) => {
                  const country = visible[index];
                  return country ? (
                    <button
                      key={country.code}
                      type="button"
                      className="country-card"
                      {...focus.bind(index)}
                      onClick={() => open(country.code)}
                    >
                      <Flag code={country.code} />
                      <strong>{countryName(country.code)}</strong>
                      <span>{country.count.toLocaleString()} channels</span>
                    </button>
                  ) : null;
                },
              )}
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
