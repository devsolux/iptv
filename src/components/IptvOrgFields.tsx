import type { IptvOrgCatalog } from '../lib/contracts';
import {
  appendCode,
  codeList,
  DEFAULT_API_BASE,
  DEFAULT_PLAYLIST_BASE,
  type IptvOrgFormValues,
  officialPlaylistPath,
  officialPlaylists,
  parseAliases,
} from '../lib/iptv-org';

interface Option {
  code: string;
  name: string;
}
function CodeField({
  id,
  label,
  hint,
  placeholder,
  value,
  options,
  disabled,
  onChange,
}: {
  id: string;
  label: string;
  hint: string;
  placeholder: string;
  value: string;
  options: Option[];
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const lookup = new Map(options.map((option) => [option.code.toLowerCase(), option.name]));
  const codes = codeList(value);
  return (
    <label htmlFor={id}>
      {label} <span className="optional">{hint}</span>
      <div className="code-field">
        <input
          data-focus
          id={id}
          aria-label={label}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
          maxLength={2048}
          autoCapitalize="none"
          spellCheck={false}
          disabled={disabled}
        />
        {options.length > 0 && (
          <select
            data-focus
            aria-label={`Add ${label.toLowerCase()}`}
            value=""
            disabled={disabled}
            onChange={(event) => {
              if (event.target.value) onChange(appendCode(value, event.target.value));
            }}
          >
            <option value="">Add…</option>
            {options.map((option) => (
              <option key={option.code} value={option.code}>
                {option.code} · {option.name}
              </option>
            ))}
          </select>
        )}
      </div>
      {codes.length > 0 && lookup.size > 0 && (
        <span className="resolved">
          {codes.map((code) => lookup.get(code.toLowerCase()) ?? code).join(' · ')}
        </span>
      )}
    </label>
  );
}
function Toggle({
  name,
  label,
  checked,
  disabled,
  onChange,
}: {
  name: string;
  label: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="checkbox-label">
      <input
        data-focus
        name={name}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span>{label}</span>
    </label>
  );
}

export function IptvOrgFields({
  values,
  onChange,
  disabled,
  catalog,
  catalogState,
}: {
  values: IptvOrgFormValues;
  onChange: (values: IptvOrgFormValues) => void;
  disabled: boolean;
  catalog: IptvOrgCatalog | null;
  catalogState: 'idle' | 'loading' | 'ready' | 'failed';
}) {
  const set = <K extends keyof IptvOrgFormValues>(key: K, value: IptvOrgFormValues[K]) =>
    onChange({ ...values, [key]: value });
  const countries: Option[] =
    catalog?.countries.map((c) => ({ code: c.code, name: `${c.flag} ${c.name}`.trim() })) ?? [];
  const categories: Option[] = catalog?.categories.map((c) => ({ code: c.id, name: c.name })) ?? [];
  const languages: Option[] = catalog?.languages ?? [];
  const regions: Option[] = catalog?.regions ?? [];
  const aliases = parseAliases(values.aliases) ?? {};
  const playlists = officialPlaylists(values).map((pick) => ({
    pick,
    path: officialPlaylistPath(pick.kind, pick.code, aliases),
  }));
  const catalogueMode = values.method === 'catalogue';
  return (
    <>
      <label htmlFor="iptv-org-method">
        Selection method
        <select
          data-focus
          id="iptv-org-method"
          className="full"
          value={values.method}
          disabled={disabled}
          onChange={(event) => set('method', event.target.value as IptvOrgFormValues['method'])}
        >
          <option value="playlist">Official playlists (country, category, language…)</option>
          <option value="catalogue">Custom selection from the JSON datasets</option>
        </select>
      </label>
      <p className="field-help">
        {catalogueMode
          ? 'Downloads the public channel, feed, stream and geography datasets, then applies one deterministic policy: country mode, categories and languages combine with AND; values within a field with OR. Adult, blocklisted and closed channels are excluded by default.'
          : 'Fetches the prebuilt playlists published by iptv-org. Country playlists follow broadcast coverage, not channel origin. Several selections are merged into one source.'}
      </p>
      {catalogState === 'loading' && (
        <p className="field-help">Loading the iptv-org country, category and language lists…</p>
      )}
      {catalogState === 'failed' && (
        <p className="field-help">
          The iptv-org lists could not be loaded; type codes directly (for example DE, news, deu).
        </p>
      )}
      <CodeField
        id="iptv-org-countries"
        label="Countries"
        hint={
          catalogueMode
            ? 'Required · two-letter codes, GB is accepted as UK'
            : 'Optional · countries/{code}.m3u; GB maps to UK, INT and UNDEFINED are special'
        }
        placeholder="DE, TR, US"
        value={values.countries}
        options={countries}
        disabled={disabled}
        onChange={(value) => set('countries', value)}
      />
      {catalogueMode && (
        <>
          <label htmlFor="iptv-org-mode">
            Country mode
            <select
              data-focus
              id="iptv-org-mode"
              className="full"
              value={values.mode}
              disabled={disabled}
              onChange={(event) => set('mode', event.target.value as IptvOrgFormValues['mode'])}
            >
              <option value="coverage-strict">Direct coverage (country, subdivision, city)</option>
              <option value="coverage-with-regions">Coverage including regions (EU, EUR…)</option>
              <option value="origin">Origin: channels from the selected countries</option>
            </select>
          </label>
          <Toggle
            name="org-international"
            label="Include worldwide and international feeds (r/WW, r/INT)"
            checked={values.includeInternational}
            disabled={disabled}
            onChange={(value) => set('includeInternational', value)}
          />
        </>
      )}
      <CodeField
        id="iptv-org-categories"
        label="Categories"
        hint={catalogueMode ? 'Optional · any of these' : 'Optional · categories/{id}.m3u'}
        placeholder="news, sports"
        value={values.categories}
        options={categories}
        disabled={disabled}
        onChange={(value) => set('categories', value)}
      />
      <CodeField
        id="iptv-org-languages"
        label="Languages"
        hint={
          catalogueMode
            ? 'Optional · ISO 639-3 codes such as deu, tur, eng'
            : 'Optional · languages/{code}.m3u with ISO 639-3 codes'
        }
        placeholder="deu, tur, eng"
        value={values.languages}
        options={languages}
        disabled={disabled}
        onChange={(value) => set('languages', value)}
      />
      {!catalogueMode && (
        <>
          <CodeField
            id="iptv-org-regions"
            label="Regions"
            hint="Optional · regions/{code}.m3u"
            placeholder="EUR, MENA"
            value={values.regions}
            options={regions}
            disabled={disabled}
            onChange={(value) => set('regions', value)}
          />
          <CodeField
            id="iptv-org-subdivisions"
            label="Subdivisions"
            hint="Optional · ISO 3166-2 codes such as DE-NW or GB-WLS, kept as published"
            placeholder="DE-NW"
            value={values.subdivisions}
            options={[]}
            disabled={disabled}
            onChange={(value) => set('subdivisions', value)}
          />
          <CodeField
            id="iptv-org-cities"
            label="Cities"
            hint="Optional · city codes such as USNYC"
            placeholder="USNYC"
            value={values.cities}
            options={[]}
            disabled={disabled}
            onChange={(value) => set('cities', value)}
          />
          <CodeField
            id="iptv-org-sources"
            label="Source playlists"
            hint="Optional · upstream file names such as us_pluto"
            placeholder="us_pluto"
            value={values.sources}
            options={[]}
            disabled={disabled}
            onChange={(value) => set('sources', value)}
          />
          <label htmlFor="iptv-org-master">
            Master playlist <span className="optional">Optional · one large playlist</span>
            <select
              data-focus
              id="iptv-org-master"
              className="full"
              value={values.master}
              disabled={disabled}
              onChange={(event) => set('master', event.target.value as IptvOrgFormValues['master'])}
            >
              <option value="">None</option>
              <option value="master">index.m3u · every channel</option>
              <option value="index-country">index.country.m3u · grouped by country</option>
              <option value="index-category">index.category.m3u · grouped by category</option>
              <option value="index-language">index.language.m3u · grouped by language</option>
            </select>
          </label>
          <p className="field-help" aria-live="polite">
            {playlists.length
              ? `Playlists to import: ${playlists
                  .map(({ pick, path }) => path ?? `invalid ${pick.kind} “${pick.code}”`)
                  .join(' · ')}`
              : 'Choose at least one country, category, language, region, subdivision, city, source or master playlist.'}
          </p>
        </>
      )}
      {catalogueMode && (
        <>
          <div className="option-grid">
            <Toggle
              name="org-nsfw"
              label="Exclude adult (NSFW) channels"
              checked={values.excludeNsfw}
              disabled={disabled}
              onChange={(value) => set('excludeNsfw', value)}
            />
            <Toggle
              name="org-blocked"
              label="Exclude blocklisted channels (DMCA, NSFW)"
              checked={values.excludeBlocked}
              disabled={disabled}
              onChange={(value) => set('excludeBlocked', value)}
            />
            <Toggle
              name="org-closed"
              label="Exclude closed channels"
              checked={values.excludeClosed}
              disabled={disabled}
              onChange={(value) => set('excludeClosed', value)}
            />
            <Toggle
              name="org-geo"
              label="Exclude streams labelled Geo-blocked"
              checked={values.excludeGeoBlockedLabel}
              disabled={disabled}
              onChange={(value) => set('excludeGeoBlockedLabel', value)}
            />
            <Toggle
              name="org-not247"
              label="Exclude streams labelled Not 24/7"
              checked={values.excludeNot247Label}
              disabled={disabled}
              onChange={(value) => set('excludeNot247Label', value)}
            />
            <Toggle
              name="org-alternatives"
              label="Keep every alternative stream as a variant"
              checked={values.includeEveryAlternativeStream}
              disabled={disabled}
              onChange={(value) => set('includeEveryAlternativeStream', value)}
            />
            <Toggle
              name="org-vlc"
              label="Pass referrer and user-agent playback hints"
              checked={values.emitVlcHttpOptions}
              disabled={disabled}
              onChange={(value) => set('emitVlcHttpOptions', value)}
            />
          </div>
          <label htmlFor="iptv-org-group">
            Group channels by
            <select
              data-focus
              id="iptv-org-group"
              className="full"
              value={values.groupBy}
              disabled={disabled}
              onChange={(event) =>
                set('groupBy', event.target.value as IptvOrgFormValues['groupBy'])
              }
            >
              <option value="country-category">Country / Category</option>
              <option value="country">Country</option>
              <option value="category">Category</option>
            </select>
          </label>
          <label htmlFor="iptv-org-refresh">
            Refresh metadata every <span className="optional">Hours · 1 to 168</span>
            <input
              data-focus
              id="iptv-org-refresh"
              type="number"
              inputMode="numeric"
              min={1}
              max={168}
              value={values.refreshHours}
              disabled={disabled}
              onChange={(event) => set('refreshHours', event.target.value)}
            />
          </label>
        </>
      )}
      <label htmlFor="iptv-org-aliases">
        Country aliases <span className="optional">Optional · one GB=UK style line each</span>
        <textarea
          data-focus
          id="iptv-org-aliases"
          rows={2}
          maxLength={2048}
          value={values.aliases}
          disabled={disabled}
          autoCapitalize="characters"
          spellCheck={false}
          onChange={(event) => set('aliases', event.target.value)}
        />
      </label>
      <label htmlFor="iptv-org-api">
        API base URL <span className="optional">Advanced · leave empty for {DEFAULT_API_BASE}</span>
        <input
          data-focus
          id="iptv-org-api"
          type="url"
          value={values.apiBase}
          placeholder={DEFAULT_API_BASE}
          maxLength={512}
          autoCapitalize="none"
          spellCheck={false}
          disabled={disabled}
          onChange={(event) => set('apiBase', event.target.value)}
        />
      </label>
      <label htmlFor="iptv-org-playlists">
        Playlist base URL{' '}
        <span className="optional">Advanced · leave empty for {DEFAULT_PLAYLIST_BASE}</span>
        <input
          data-focus
          id="iptv-org-playlists"
          type="url"
          value={values.playlistBase}
          placeholder={DEFAULT_PLAYLIST_BASE}
          maxLength={512}
          autoCapitalize="none"
          spellCheck={false}
          disabled={disabled}
          onChange={(event) => set('playlistBase', event.target.value)}
        />
      </label>
    </>
  );
}
