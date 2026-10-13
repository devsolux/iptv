export type Direction = 'ArrowUp' | 'ArrowDown' | 'ArrowLeft' | 'ArrowRight';
export function isDirection(key: string): key is Direction {
  return key === 'ArrowUp' || key === 'ArrowDown' || key === 'ArrowLeft' || key === 'ArrowRight';
}
/** null means the current focus scope owns no neighbour in this direction. */
export function gridNeighbour(
  index: number,
  count: number,
  columns: number,
  direction: Direction,
): number | null {
  if (count < 1 || columns < 1 || index < 0 || index >= count) return null;
  const col = index % columns;
  switch (direction) {
    case 'ArrowLeft':
      return col > 0 ? index - 1 : null;
    case 'ArrowRight':
      return col + 1 < columns && index + 1 < count ? index + 1 : null;
    case 'ArrowUp':
      return index >= columns ? index - columns : null;
    case 'ArrowDown':
      return index + columns < count
        ? index + columns
        : Math.floor(index / columns) < Math.floor((count - 1) / columns)
          ? count - 1
          : null;
  }
}
export interface TimedItem {
  start: number;
  end: number;
}
export function nearestAtTime<T extends TimedItem>(
  items: readonly T[],
  time: number,
): T | undefined {
  let selected: T | undefined;
  let distance = Number.POSITIVE_INFINITY;
  for (const item of items) {
    const current =
      time < item.start ? item.start - time : time >= item.end ? time - item.end + 1 : 0;
    if (
      current < distance ||
      (current === distance && item.start < (selected?.start ?? Infinity))
    ) {
      distance = current;
      selected = item;
    }
  }
  return selected;
}
/** A deterministic, bounded presentation of overlapping provider programmes. */
export function timelineSpans<T extends TimedItem>(
  items: readonly T[],
  start: number,
  end: number,
): { item: T; left: number; width: number }[] {
  if (end <= start) return [];
  const sorted = [...items]
    .filter((item) => item.end > start && item.start < end)
    .sort((a, b) => a.start - b.start || a.end - b.end);
  const spans: { item: T; left: number; width: number }[] = [];
  for (let index = 0; index < sorted.length; index++) {
    const item = sorted[index];
    if (!item) continue;
    const visibleStart = Math.max(start, item.start);
    const visibleEnd = Math.min(end, item.end, sorted[index + 1]?.start ?? end);
    if (visibleEnd <= visibleStart) continue;
    spans.push({
      item,
      left: ((visibleStart - start) / (end - start)) * 100,
      width: ((visibleEnd - visibleStart) / (end - start)) * 100,
    });
  }
  return spans;
}
