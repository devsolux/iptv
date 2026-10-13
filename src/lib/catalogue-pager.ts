import type { Channel, ChannelFilters, ChannelPage, ChannelQuery } from './contracts';

export const PAGE_SIZE = 80;
const MAX_PAGES = 6;
export interface CatalogueSnapshot {
  pages: ReadonlyMap<number, ChannelPage>;
  total: number;
  revision: number | null;
  loading: boolean;
  ready: boolean;
  error: unknown;
}

/** One bounded query/cache owner shared by the grid and guide. No catalogue parsing. */
export class CataloguePager {
  private readonly load: (query: ChannelQuery) => Promise<ChannelPage>;
  private filters: ChannelFilters = {
    search: '',
    sourceId: null,
    group: null,
    country: null,
    favoritesOnly: false,
  };
  private generation = 0;
  private sequence = 0;
  private active = new Map<number, { generation: number; page: number }>();
  private wanted: number[] = [0];
  private cache = new Map<number, ChannelPage>();
  private listeners = new Set<() => void>();
  private retries = 0;
  private disposed = false;
  private snapshot: CatalogueSnapshot = {
    pages: new Map(),
    total: 0,
    revision: null,
    loading: false,
    ready: false,
    error: null,
  };

  constructor(load: (query: ChannelQuery) => Promise<ChannelPage>) {
    this.load = load;
  }
  getSnapshot = (): CatalogueSnapshot => this.snapshot;
  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  at(index: number): Channel | undefined {
    return this.cache.get(Math.floor(index / PAGE_SIZE))?.items[index % PAGE_SIZE];
  }
  reset(filters: ChannelFilters, initialIndex = 0): void {
    this.filters = { ...filters };
    this.retries = 0;
    this.restart(initialIndex);
  }
  private restart(initialIndex: number): void {
    this.disposed = false;
    this.generation++;
    this.cache.clear();
    this.wanted = [Math.max(0, Math.floor(initialIndex / PAGE_SIZE))];
    this.snapshot = {
      pages: new Map(),
      total: 0,
      revision: null,
      loading: true,
      ready: false,
      error: null,
    };
    this.emit();
    this.pump();
  }
  wantRange(first: number, last: number): void {
    const start = Math.max(0, Math.floor(first / PAGE_SIZE));
    const end = Math.min(start + 2, Math.max(start, Math.floor(last / PAGE_SIZE)));
    this.wanted = Array.from({ length: end - start + 1 }, (_, i) => start + i);
    this.pump();
  }
  private emit(): void {
    this.snapshot = {
      ...this.snapshot,
      pages: new Map(this.cache),
      loading:
        [...this.active.values()].some((request) => request.generation === this.generation) ||
        !this.snapshot.ready,
    };
    for (const listener of this.listeners) listener();
  }
  private pump(): void {
    if (this.disposed || this.snapshot.error) return;
    while (this.active.size < 2) {
      const page = this.wanted.find(
        (p) =>
          !this.cache.has(p) &&
          ![...this.active.values()].some(
            (request) => request.generation === this.generation && request.page === p,
          ),
      );
      if (page === undefined) break;
      const generation = this.generation;
      const ticket = ++this.sequence;
      this.active.set(ticket, { generation, page });
      this.emit();
      const query = {
        ...this.filters,
        offset: page * PAGE_SIZE,
        limit: PAGE_SIZE,
        revision: this.snapshot.revision,
      };
      void this.load(query)
        .then((result) => {
          if (generation !== this.generation || this.disposed) return;
          if (this.snapshot.revision !== null && this.snapshot.revision !== result.revision)
            throw { code: 'STALE' };
          this.cache.delete(page);
          this.cache.set(page, result);
          while (this.cache.size > MAX_PAGES) {
            const victim = [...this.cache.keys()].find((key) => !this.wanted.includes(key));
            if (victim === undefined) break;
            this.cache.delete(victim);
          }
          this.snapshot = {
            ...this.snapshot,
            total: result.total,
            revision: result.revision,
            ready: true,
            error: null,
          };
        })
        .catch((error: unknown) => {
          if (generation !== this.generation || this.disposed) return;
          if (
            typeof error === 'object' &&
            error !== null &&
            'code' in error &&
            error.code === 'STALE' &&
            this.retries++ < 2
          ) {
            this.restart((this.wanted[0] ?? 0) * PAGE_SIZE);
          } else {
            this.snapshot = { ...this.snapshot, ready: true, error };
          }
        })
        .finally(() => {
          this.active.delete(ticket);
          if (!this.disposed) {
            this.emit();
            this.pump();
          }
        });
    }
  }
  dispose(): void {
    this.disposed = true;
    this.generation++;
    this.wanted = [];
    this.listeners.clear();
  }
}
