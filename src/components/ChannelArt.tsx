import { useState } from 'react';

/** Channel artwork from the loopback relay with a monogram fallback; never a remote URL. */
export function ChannelArt({
  name,
  logoUrl,
  compact = false,
}: {
  name: string;
  logoUrl: string | null;
  compact?: boolean;
}) {
  const [failed, setFailed] = useState(false);
  if (!logoUrl || failed) {
    if (compact) return null;
    return (
      <div className="channel-monogram" aria-hidden="true">
        {name
          .replace(/^Channel /, '')
          .slice(0, 3)
          .toUpperCase()}
      </div>
    );
  }
  return (
    <img
      className={compact ? 'channel-logo compact' : 'channel-logo'}
      src={logoUrl}
      alt=""
      loading="lazy"
      decoding="async"
      onError={() => setFailed(true)}
    />
  );
}
