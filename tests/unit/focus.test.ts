import assert from 'node:assert/strict';
import { test } from 'node:test';
import { gridNeighbour, isDirection, nearestAtTime, timelineSpans } from '../../src/lib/focus.ts';

test('grid navigation does not wrap rows or escape an empty scope', () => {
  assert.equal(gridNeighbour(0, 10, 4, 'ArrowLeft'), null);
  assert.equal(gridNeighbour(3, 10, 4, 'ArrowRight'), null);
  assert.equal(gridNeighbour(0, 0, 4, 'ArrowDown'), null);
  assert.equal(gridNeighbour(2, 10, 4, 'ArrowRight'), 3);
  assert.equal(gridNeighbour(7, 10, 4, 'ArrowDown'), 9);
  assert.equal(gridNeighbour(9, 10, 4, 'ArrowDown'), null);
  assert.equal(gridNeighbour(9, 10, 4, 'ArrowUp'), 5);
});
test('all grid neighbours remain valid over varied row widths and partial last rows', () => {
  for (let count = 1; count < 160; count++) for (let columns = 1; columns <= 8; columns++) {
    for (let index = 0; index < count; index++) for (const direction of ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'] as const) {
      const next = gridNeighbour(index, count, columns, direction);
      assert.ok(next === null || Number.isInteger(next) && next >= 0 && next < count && next !== index);
      if (next !== null && (direction === 'ArrowLeft' || direction === 'ArrowRight')) assert.equal(Math.floor(next / columns), Math.floor(index / columns));
    }
  }
});
test('remote key classification excludes text input and unrelated keys', () => {
  assert.equal(isDirection('ArrowLeft'), true); assert.equal(isDirection('Enter'), false); assert.equal(isDirection('a'), false);
});
test('guide navigation preserves time and prefers the earlier item at an equal distance', () => {
  const items = [{ start: 0, end: 100 }, { start: 120, end: 200 }, { start: 200, end: 300 }];
  assert.equal(nearestAtTime(items, 50), items[0]); assert.equal(nearestAtTime(items, 200), items[2]);
  assert.equal(nearestAtTime(items, 110), items[1]); assert.equal(nearestAtTime([], 0), undefined);
});
test('guide layout clips to its time window and removes overlapping pixels deterministically', () => {
  const input = [{ start: -20, end: 60 }, { start: 40, end: 80 }, { start: 80, end: 140 }];
  const spans = timelineSpans(input, 0, 100);
  assert.deepEqual(spans.map(({ left, width }) => [left, width]), [[0, 40], [40, 40], [80, 20]]);
  assert.equal(input[0]?.start, -20); assert.deepEqual(timelineSpans(input, 100, 100), []);
});
