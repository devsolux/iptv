import type { Channel, ChannelQuery, GuideQuery, Programme } from './contracts';

const sourceId = '00000000-0000-4000-8000-000000000001';
const regions = ['DE', 'TR', 'US', 'GB', 'FR', 'JP', 'BR'];
const groups = ['News', 'Documentary', 'Entertainment', 'Sports', 'Music'];
const count = 50_000;
function channel(index: number): Channel {
  return {
    id: `10000000-0000-4000-8000-${String(index + 1).padStart(12, '0')}`,
    sourceId,
    name: `Channel ${String(index + 1).padStart(5, '0')}`,
    country: regions[index % regions.length] ?? 'DE',
    favorite: index % 13 === 0,
    variants: 1,
    hasEpg: index % 7 !== 0,
    logoUrl: null,
  };
}
function channels(query: ChannelQuery) {
  const items: Channel[] = [];
  let total = 0;
  // Synthetic development data only; no catalogue is retained or parsed in the browser.
  for (let index = 0; index < count; index++) {
    const item = channel(index);
    if (query.sourceId && query.sourceId !== sourceId) continue;
    if (query.group && groups[index % groups.length] !== query.group) continue;
    if (query.country && item.country !== query.country) continue;
    if (query.favoritesOnly && !item.favorite) continue;
    if (query.search && !item.name.toLowerCase().includes(query.search.toLowerCase())) continue;
    if (total >= query.offset && items.length < query.limit) items.push(item);
    total++;
  }
  return { items, total, revision: 1, offset: query.offset };
}
function guide(query: GuideQuery) {
  const items: Programme[] = [];
  const start = Math.floor(query.start / 1800) * 1800;
  for (const id of query.channelIds) {
    const index = Number(id.slice(-12)) - 1;
    if (index % 7 === 0) continue;
    for (let time = start; time < query.end; time += 1800) {
      items.push({
        channelId: id,
        start: time,
        end: time + 1800,
        title:
          ['World Report', 'On the Road', 'The Big Picture', 'Live Studio'][
            (Math.floor(time / 1800) + index) % 4
          ] ?? 'Live Studio',
        description:
          'Synthetic programme information for development and remote-navigation testing.',
      });
    }
  }
  return { items, revision: 1 };
}
export async function previewCommand(
  command: string,
  args: Record<string, unknown>,
): Promise<unknown> {
  await new Promise((resolve) => setTimeout(resolve, 35));
  switch (command) {
    case 'app_status':
      return {
        name: 'IPTV',
        version: '1.0.0',
        identifier: 'com.devsolux.iptv',
        unlocked: true,
        vaultExists: true,
        nativePlayer: false,
      };
    case 'list_sources':
      return [
        {
          id: sourceId,
          name: 'Development catalogue',
          kind: 'm3u',
          channels: count,
          updatedAt: Math.floor(Date.now() / 1000),
          hasEpg: true,
        },
      ];
    case 'list_jobs':
      return [];
    case 'query_countries':
      return regions.map((code, index) => ({
        code,
        count: Math.floor((count + regions.length - index - 1) / regions.length),
      }));
    case 'locate_channel': {
      const query = args.query as ChannelQuery;
      const fullIndex = Number(String(args.channelId).slice(-12)) - 1;
      // Locate without creating a full synthetic catalogue array.
      let position = 0;
      for (let index = 0; index <= fullIndex; index++) {
        const item = channel(index);
        if (query.country && query.country !== item.country) continue;
        if (query.group && groups[index % groups.length] !== query.group) continue;
        if (query.favoritesOnly && !item.favorite) continue;
        if (query.search && !item.name.toLowerCase().includes(query.search.toLowerCase())) continue;
        if (index === fullIndex) return { index: position, revision: 1 };
        position++;
      }
      return { index: null, revision: 1 };
    }
    case 'query_channels':
      return channels(args.query as ChannelQuery);
    case 'query_groups':
      return groups;
    case 'iptv_org_catalog':
      return {
        countries: regions.map((code) => ({
          code,
          name: code,
          flag: '',
          officialLanguageCodes: [],
          playlistCandidateUrl: `https://iptv-org.github.io/iptv/countries/${code.toLowerCase()}.m3u`,
        })),
        categories: groups.map((name) => ({ id: name.toLowerCase(), name })),
        languages: [
          { code: 'eng', name: 'English' },
          { code: 'deu', name: 'German' },
          { code: 'tur', name: 'Turkish' },
        ],
        regions: [{ code: 'EUR', name: 'Europe', countries: ['DE', 'FR', 'GB', 'TR'] }],
      };
    case 'query_guide':
      return guide(args.query as GuideQuery);
    default:
      throw {
        code: 'PREVIEW',
        message:
          'This is a read-only preview. Start the Tauri application for imports, storage and playback.',
      };
  }
}
