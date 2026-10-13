import { useState } from 'react';
export function countryName(code: string): string {
  try { return new Intl.DisplayNames(['en'], { type: 'region' }).of(code.toUpperCase()) ?? code; }
  catch { return code; }
}
export function Flag({ code }: { code: string }) {
  const [failed, setFailed] = useState(false);
  const safe = /^[a-z]{2}$/i.test(code);
  if (!safe || failed) return <span className="country-fallback" aria-hidden="true">{code.slice(0, 2) || 'TV'}</span>;
  return <img className="flag" src={`/flags/${code.toLowerCase()}.svg`} alt="" loading="lazy" decoding="async" onError={() => setFailed(true)} />;
}
