const paths = {
  channels: 'M3 4h18v13H3zM8 21h8M12 17v4',
  guide: 'M4 3h16v18H4zM4 8h16M9 8v13M9 13h11M9 17h11',
  globe: 'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0ZM3 12h18M12 3c5 5 5 13 0 18-5-5-5-13 0-18Z',
  star: 'm12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9L12 3Z',
  sources: 'M4 7h16v14H4zM8 7V3h8v4M8 12h8M8 16h5',
  lock: 'M6 10h12v11H6zM8 10V6a4 4 0 0 1 8 0v4M12 14v3',
  search: 'M16 10a6 6 0 1 1-12 0 6 6 0 0 1 12 0Zm-1 5 6 6',
  play: 'm8 4 12 8-12 8V4Z',
  back: 'm14 5-7 7 7 7',
  refresh: 'M20 7a9 9 0 1 0 1 8M20 2v6h-6',
  close: 'm5 5 14 14M19 5 5 19',
  more: 'M5 11h1v2H5zM11 11h1v2h-1zM17 11h1v2h-1z',
  pause: 'M7 5h3v14H7zM14 5h3v14h-3z',
  rewind: 'M11 5a7 7 0 1 1-6.3 4M4 4v5h5',
  forward: 'M13 5a7 7 0 1 0 6.3 4M20 4v5h-5',
  live: 'M12 10a2 2 0 1 0 0 4 2 2 0 0 0 0-4ZM8.5 8.5a5 5 0 0 0 0 7M15.5 8.5a5 5 0 0 1 0 7M5.6 5.6a9 9 0 0 0 0 12.8M18.4 5.6a9 9 0 0 1 0 12.8',
  volume: 'M4 9v6h4l5 4V5L8 9H4ZM16 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11',
  volumeOff: 'M4 9v6h4l5 4V5L8 9H4ZM17 9l4 6M21 9l-4 6',
  fullscreen: 'M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5',
  fullscreenExit: 'M9 4v5H4M15 4v5h5M20 15h-5v5M4 15h5v5',
} as const;
export function Icon({ name, size = 22 }: { name: keyof typeof paths; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.65"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[name]} />
    </svg>
  );
}
