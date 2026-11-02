// Game logic for the "Scam fire drill" practice page (see practice.html).
// Pure functions only: no DOM, no timers, no locale lookups.
// src/practice.ts maps flag ids and ClickResults to locale strings.

export type RoundId = 'prize' | 'bank' | 'virus';

/** Something the person can click on a fictional scam page. */
export interface Target {
  id: string;
  /** 'flag' = one of the round's red flags; 'decoy' = a fine part of the page. */
  kind: 'flag' | 'decoy';
  /** Set when kind === 'flag'; must be one of the current round's flags. */
  flagId?: string;
}

export interface RoundDef {
  id: RoundId;
  /** Exactly three red flags hidden in this round. */
  flags: readonly string[];
}

export const FLAGS_PER_ROUND = 3;

export const ROUNDS: readonly RoundDef[] = [
  { id: 'prize', flags: ['countdown', 'free-prize', 'pay-fee'] },
  { id: 'bank', flags: ['sender-address', 'urgency', 'signin-link'] },
  { id: 'virus', flags: ['virus-count', 'phone-number', 'call-button'] },
];

export const TOTAL_FLAGS = ROUNDS.reduce((sum, round) => sum + round.flags.length, 0);

export type Phase = 'playing' | 'revealed' | 'finished';

export interface PracticeState {
  readonly roundIndex: number;
  readonly found: readonly string[];
  readonly phase: Phase;
  readonly totalFound: number;
  readonly misses: number;
}

export type ClickResult = 'spotted' | 'already-spotted' | 'fine' | 'locked';

export function createState(): PracticeState {
  return { roundIndex: 0, found: [], phase: 'playing', totalFound: 0, misses: 0 };
}

export function currentRound(state: PracticeState): RoundDef {
  return ROUNDS[state.roundIndex];
}

/** Click a target on the current round's page. Returns the new state and what happened. */
export function click(state: PracticeState, target: Target): { state: PracticeState; result: ClickResult } {
  if (state.phase !== 'playing') {
    return { state, result: 'locked' };
  }
  if (target.kind === 'decoy') {
    return { state: { ...state, misses: state.misses + 1 }, result: 'fine' };
  }
  const flagId = target.flagId ?? '';
  if (!currentRound(state).flags.includes(flagId)) {
    // Unknown flag id for this round: count as a harmless wrong click.
    return { state: { ...state, misses: state.misses + 1 }, result: 'fine' };
  }
  if (state.found.includes(flagId)) {
    return { state, result: 'already-spotted' };
  }
  const found = [...state.found, flagId];
  return {
    state: {
      ...state,
      found,
      totalFound: state.totalFound + 1,
      phase: found.length >= FLAGS_PER_ROUND ? 'revealed' : 'playing',
    },
    result: 'spotted',
  };
}

/** "Show me": reveal all flags of the current round without scoring them. */
export function showMe(state: PracticeState): PracticeState {
  if (state.phase !== 'playing') {
    return state;
  }
  return { ...state, phase: 'revealed' };
}

/** Move to the next round, or finish after the last one. Only valid when revealed. */
export function nextRound(state: PracticeState): PracticeState {
  if (state.phase !== 'revealed') {
    return state;
  }
  const roundIndex = state.roundIndex + 1;
  if (roundIndex >= ROUNDS.length) {
    return { ...state, roundIndex, phase: 'finished' };
  }
  return { ...state, roundIndex, found: [], phase: 'playing' };
}

export function score(state: PracticeState): { found: number; total: number } {
  return { found: state.totalFound, total: TOTAL_FLAGS };
}
