import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  CODEC,
  dashErrorCode,
  dashFailure,
  FORMAT,
  LOST,
  mediaFailure,
  POLICY,
  PROTECTED,
  permanentRelayFailure,
  ReconnectBudget,
  relayFailure,
  TOO_LARGE,
} from '../../src/lib/playback.ts';

test('relay statuses map to the failure they stand for', () => {
  assert.equal(relayFailure(403), POLICY);
  assert.equal(relayFailure(413), TOO_LARGE);
  assert.equal(relayFailure(415), FORMAT);
  for (const status of [0, 410, 502, 503]) assert.equal(relayFailure(status), LOST);
  assert.ok(permanentRelayFailure(403) && permanentRelayFailure(415));
  assert.ok(
    !permanentRelayFailure(0) && !permanentRelayFailure(410) && !permanentRelayFailure(502),
  );
});

test('dash.js clock and subtitle errors are not fatal', () => {
  // A live MPD whose clock source is unreachable used to stop playback as a codec failure.
  assert.equal(dashFailure(16), null);
  assert.equal(dashFailure(33), null);
  assert.equal(dashFailure(21), null);
  assert.equal(dashFailure(25), LOST);
  assert.equal(dashFailure(27), LOST);
  assert.equal(dashFailure(31), FORMAT);
  assert.equal(dashFailure(35), CODEC);
  assert.equal(dashFailure(24), PROTECTED);
  assert.equal(dashFailure(111), PROTECTED);
  assert.equal(dashFailure(null), FORMAT);
});

test('dash.js error codes are read from object and legacy events', () => {
  assert.equal(dashErrorCode({ error: { code: 16, message: 'sync' } }), 16);
  assert.equal(dashErrorCode({ error: 'download', event: {} }), null);
  assert.equal(dashErrorCode(null), null);
  assert.equal(dashErrorCode({ error: { code: '16' } }), null);
});

test('media element errors keep distinct messages', () => {
  assert.equal(mediaFailure(2), LOST);
  assert.equal(mediaFailure(3), CODEC);
  assert.equal(mediaFailure(4), FORMAT);
  assert.match(mediaFailure(undefined), /unexpectedly/);
});

test('reconnection is bounded per sliding window', () => {
  const budget = new ReconnectBudget(3, 1000);
  assert.ok(budget.take(0));
  assert.ok(budget.take(100));
  assert.ok(budget.take(200));
  assert.ok(!budget.take(300));
  // Attempts older than the window no longer count.
  assert.ok(budget.take(1050));
  assert.ok(!budget.take(1060));
  budget.reset();
  assert.ok(budget.take(1060));
});
