import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  ROUNDS,
  TOTAL_FLAGS,
  click,
  createState,
  currentRound,
  nextRound,
  score,
  showMe,
  type Target,
} from './logic.ts';

const flag = (flagId: string): Target => ({ id: flagId, kind: 'flag', flagId });
const decoy = (id: string): Target => ({ id, kind: 'decoy' });

test('createState starts the first round with no flags found', () => {
  const state = createState();
  assert.equal(state.roundIndex, 0);
  assert.equal(state.phase, 'playing');
  assert.deepEqual(state.found, []);
  assert.equal(state.totalFound, 0);
  assert.equal(currentRound(state).id, ROUNDS[0].id);
});

test('clicking a decoy says fine and counts a miss without scoring', () => {
  const { state, result } = click(createState(), decoy('prize-nothanks'));
  assert.equal(result, 'fine');
  assert.equal(state.misses, 1);
  assert.equal(state.totalFound, 0);
  assert.equal(state.phase, 'playing');
});

test('clicking a red flag says spotted and marks it found', () => {
  const { state, result } = click(createState(), flag('countdown'));
  assert.equal(result, 'spotted');
  assert.deepEqual(state.found, ['countdown']);
  assert.equal(state.totalFound, 1);
});

test('clicking the same flag twice does not double count', () => {
  const once = click(createState(), flag('countdown')).state;
  const { state, result } = click(once, flag('countdown'));
  assert.equal(result, 'already-spotted');
  assert.equal(state.totalFound, 1);
});

test('finding all three flags reveals the round automatically', () => {
  let state = createState();
  for (const flagId of ROUNDS[0].flags) {
    state = click(state, flag(flagId)).state;
  }
  assert.equal(state.phase, 'revealed');
  assert.equal(state.totalFound, 3);
});

test('showMe reveals without scoring the remaining flags', () => {
  const { state } = click(createState(), flag('countdown'));
  const revealed = showMe(state);
  assert.equal(revealed.phase, 'revealed');
  assert.equal(revealed.totalFound, 1);
});

test('showMe is a no-op after the round is revealed', () => {
  const { state } = click(createState(), flag('countdown'));
  const revealed = showMe(showMe(state));
  assert.equal(revealed.totalFound, 1);
});

test('nextRound moves to the next round and keeps the score', () => {
  const { state } = click(createState(), flag('countdown'));
  const next = nextRound(showMe(state));
  assert.equal(next.roundIndex, 1);
  assert.equal(next.phase, 'playing');
  assert.deepEqual(next.found, []);
  assert.equal(next.totalFound, 1);
});

test('nextRound is a no-op while still playing', () => {
  const state = createState();
  assert.equal(nextRound(state), state);
});

test('finishing the last round sets phase finished', () => {
  let state = createState();
  for (let i = 0; i < ROUNDS.length; i += 1) {
    state = nextRound(showMe(state));
  }
  assert.equal(state.phase, 'finished');
  assert.equal(state.roundIndex, ROUNDS.length);
});

test('a full perfect run scores 9 of 9', () => {
  let state = createState();
  for (let i = 0; i < ROUNDS.length; i += 1) {
    for (const flagId of currentRound(state).flags) {
      state = click(state, flag(flagId)).state;
    }
    assert.equal(state.phase, 'revealed');
    state = nextRound(state);
  }
  assert.equal(state.phase, 'finished');
  assert.deepEqual(score(state), { found: TOTAL_FLAGS, total: 9 });
});

test('score exposes found and total', () => {
  assert.deepEqual(score(createState()), { found: 0, total: 9 });
  assert.equal(TOTAL_FLAGS, 9);
});

test('clicks are locked after reveal and after finish', () => {
  const revealed = showMe(createState());
  const { state, result } = click(revealed, flag('countdown'));
  assert.equal(result, 'locked');
  assert.equal(state.totalFound, 0);

  let finished = createState();
  for (let i = 0; i < ROUNDS.length; i += 1) {
    finished = nextRound(showMe(finished));
  }
  assert.equal(click(finished, decoy('prize-nothanks')).result, 'locked');
});

test('a flag id from another round counts as a fine miss', () => {
  const { state, result } = click(createState(), flag('sender-address'));
  assert.equal(result, 'fine');
  assert.equal(state.misses, 1);
  assert.equal(state.totalFound, 0);
});
