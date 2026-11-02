import "./practice.css";
import { getLocale, t, type LocaleKey } from "./i18n/index.ts";
import { followSettingsLocale } from "./i18n/settings.ts";
import {
  ROUNDS,
  click,
  createState,
  currentRound,
  nextRound,
  score,
  showMe,
  type PracticeState,
  type RoundId,
  type Target,
} from "./practice/logic.ts";

const appRoot = document.getElementById('app');
if (!appRoot) {
  throw new Error('practice page: missing #app element');
}
const root: HTMLElement = appRoot;

interface PartSpec {
  id: string;
  className: string;
  text: string;
  target: Target;
  explKey?: LocaleKey;
}

const ROUND_TITLES: Record<RoundId, LocaleKey> = {
  prize: 'practice.page.prize.title',
  bank: 'practice.page.bank.title',
  virus: 'practice.page.virus.title',
};

let state: PracticeState = createState();
let lastFeedbackKey: 'practice.feedback.fine' | 'practice.feedback.already' | null = null;

// A clickable part of the fictional scam page. Every part is a real <button>,
// so Tab / Enter works everywhere and targets stay big.
function part(spec: PartSpec): HTMLButtonElement {
  const el = document.createElement('button');
  el.type = 'button';
  el.className = `practice-part ${spec.className}`;
  el.dataset.partId = spec.id;
  el.dataset.kind = spec.target.kind;
  if (spec.target.flagId) {
    el.dataset.flagId = spec.target.flagId;
  }
  el.append(document.createTextNode(spec.text));
  if (spec.target.kind === 'flag' && spec.target.flagId) {
    if (state.found.includes(spec.target.flagId)) {
      el.classList.add('is-spotted');
    }
    if (state.phase === 'revealed') {
      el.classList.add('is-revealed');
    }
    const burst = document.createElement('span');
    burst.className = 'practice-burst';
    burst.textContent = t('practice.spotted');
    burst.setAttribute('aria-hidden', 'true');
    const expl = document.createElement('span');
    expl.className = 'practice-expl';
    expl.textContent = spec.explKey ? t(spec.explKey) : '';
    el.append(burst, expl);
  }
  return el;
}

// A plain, non-clickable line of the fictional page.
function line(className: string, textKey: LocaleKey): HTMLParagraphElement {
  const p = document.createElement('p');
  p.className = `practice-line ${className}`;
  p.textContent = t(textKey);
  return p;
}

function pageShell(variant: string): HTMLElement {
  const page = document.createElement('div');
  page.className = `practice-page practice-page--${variant}`;
  return page;
}

// Round 1: "You won a prize!" pop-up (all fictional, no real brands).
function buildPrizePage(): HTMLElement {
  const page = pageShell('popup');
  page.append(
    part({ id: 'prize-brand', className: 'practice-line--meta', text: t('practice.page.prize.brand'), target: { id: 'prize-brand', kind: 'decoy' } }),
    part({ id: 'prize-headline', className: 'practice-line--headline', text: t('practice.page.prize.headline'), target: { id: 'prize-headline', kind: 'flag', flagId: 'free-prize' }, explKey: 'practice.flag.free-prize' }),
    part({ id: 'prize-countdown', className: 'practice-countdown', text: `${t('practice.page.prize.timerLabel')} 04:59`, target: { id: 'prize-countdown', kind: 'flag', flagId: 'countdown' }, explKey: 'practice.flag.countdown' }),
    part({ id: 'prize-pay', className: 'practice-btn practice-btn--danger', text: t('practice.page.prize.pay'), target: { id: 'prize-pay', kind: 'flag', flagId: 'pay-fee' }, explKey: 'practice.flag.pay-fee' }),
    part({ id: 'prize-nothanks', className: 'practice-btn practice-btn--quiet', text: t('practice.page.prize.noThanks'), target: { id: 'prize-nothanks', kind: 'decoy' } }),
  );
  return page;
}

// Round 2: fake bank e-mail with a look-alike sender address.
function buildBankPage(): HTMLElement {
  const page = pageShell('email');
  page.append(
    part({ id: 'bank-subject', className: 'practice-line--subject', text: t('practice.page.bank.subject'), target: { id: 'bank-subject', kind: 'flag', flagId: 'urgency' }, explKey: 'practice.flag.urgency' }),
    part({ id: 'bank-sender', className: 'practice-line--sender', text: t('practice.page.bank.sender'), target: { id: 'bank-sender', kind: 'flag', flagId: 'sender-address' }, explKey: 'practice.flag.sender-address' }),
    part({ id: 'bank-when', className: 'practice-line--meta', text: t('practice.page.bank.when'), target: { id: 'bank-when', kind: 'decoy' } }),
    line('', 'practice.page.bank.greeting'),
    line('', 'practice.page.bank.body'),
    part({ id: 'bank-signin', className: 'practice-btn', text: t('practice.page.bank.signin'), target: { id: 'bank-signin', kind: 'flag', flagId: 'signin-link' }, explKey: 'practice.flag.signin-link' }),
    part({ id: 'bank-footer', className: 'practice-line--meta', text: t('practice.page.bank.footer'), target: { id: 'bank-footer', kind: 'decoy' } }),
  );
  return page;
}

// Round 3: fake "your computer has viruses" alert with a phone number.
function buildVirusPage(): HTMLElement {
  const page = pageShell('alert');
  page.append(
    part({ id: 'virus-brand', className: 'practice-line--meta', text: t('practice.page.virus.brand'), target: { id: 'virus-brand', kind: 'decoy' } }),
    part({ id: 'virus-headline', className: 'practice-line--headline', text: t('practice.page.virus.headline'), target: { id: 'virus-headline', kind: 'flag', flagId: 'virus-count' }, explKey: 'practice.flag.virus-count' }),
    line('', 'practice.page.virus.body'),
    part({ id: 'virus-phone', className: 'practice-phone', text: t('practice.page.virus.phone'), target: { id: 'virus-phone', kind: 'flag', flagId: 'phone-number' }, explKey: 'practice.flag.phone-number' }),
    part({ id: 'virus-call', className: 'practice-btn practice-btn--danger', text: t('practice.page.virus.call'), target: { id: 'virus-call', kind: 'flag', flagId: 'call-button' }, explKey: 'practice.flag.call-button' }),
    part({ id: 'virus-scan', className: 'practice-line--meta', text: t('practice.page.virus.scan'), target: { id: 'virus-scan', kind: 'decoy' } }),
  );
  return page;
}

const ROUND_PAGES: Record<RoundId, () => HTMLElement> = {
  prize: buildPrizePage,
  bank: buildBankPage,
  virus: buildVirusPage,
};

function buildControls(): { controls: HTMLElement; nextButton: HTMLButtonElement | null } {
  const controls = document.createElement('div');
  controls.className = 'practice-controls';
  let nextButton: HTMLButtonElement | null = null;
  if (state.phase === 'playing') {
    const show = document.createElement('button');
    show.type = 'button';
    show.className = 'practice-btn practice-control';
    show.textContent = t('practice.showMe');
    show.addEventListener('click', () => {
      state = showMe(state);
      lastFeedbackKey = null;
      render('next');
    });
    controls.append(show);
  }
  if (state.phase === 'revealed') {
    nextButton = document.createElement('button');
    nextButton.type = 'button';
    nextButton.className = 'practice-btn practice-control';
    nextButton.textContent = t('practice.next');
    nextButton.addEventListener('click', () => {
      state = nextRound(state);
      lastFeedbackKey = null;
      render('title');
    });
    controls.append(nextButton);
  }
  return { controls, nextButton };
}

function renderBadge(): void {
  const result = score(state);
  const wrap = document.createElement('div');
  wrap.className = 'practice-badge-wrap';
  const badge = document.createElement('div');
  badge.className = 'practice-badge';
  badge.textContent = t('practice.result.badge');
  const scoreLine = document.createElement('p');
  scoreLine.className = 'practice-score';
  scoreLine.textContent = t('practice.result.score', { found: result.found, total: result.total });
  wrap.append(badge, scoreLine);
  if (result.found === result.total) {
    const perfect = document.createElement('p');
    perfect.className = 'practice-perfect';
    perfect.textContent = t('practice.result.perfect');
    wrap.append(perfect);
  }
  const tip = document.createElement('p');
  tip.className = 'practice-tip';
  tip.textContent = t('practice.result.tip');
  const done = document.createElement('button');
  done.type = 'button';
  done.className = 'practice-btn practice-control';
  done.textContent = t('practice.done');
  done.addEventListener('click', () => {
    window.close();
  });
  wrap.append(tip, done);
  root.append(wrap);
  done.focus();
}

function render(focusTarget: 'title' | 'next' | 'none' = 'none'): void {
  document.documentElement.lang = getLocale();
  document.title = t('practice.title');
  root.className = 'practice-root';
  root.replaceChildren();
  if (state.phase === 'finished') {
    renderBadge();
    return;
  }

  const header = document.createElement('header');
  header.className = 'practice-header';
  const h1 = document.createElement('h1');
  h1.className = 'practice-title';
  h1.textContent = t('practice.title');
  const subtitle = document.createElement('p');
  subtitle.className = 'practice-subtitle';
  subtitle.textContent = t('practice.subtitle');
  const progress = document.createElement('p');
  progress.className = 'practice-progress';
  progress.textContent = `${t('practice.progress', { current: state.roundIndex + 1, total: ROUNDS.length })} · ${t('practice.foundCount', { found: state.found.length })}`;
  header.append(h1, subtitle, progress);

  const stage = document.createElement('section');
  stage.className = 'practice-stage';
  const round = currentRound(state);
  const roundTitle = document.createElement('h2');
  roundTitle.className = 'practice-round-title';
  roundTitle.tabIndex = -1;
  roundTitle.textContent = t(ROUND_TITLES[round.id]);
  const instruction = document.createElement('p');
  instruction.className = 'practice-instruction';
  instruction.textContent = t('practice.instruction');
  const page = ROUND_PAGES[round.id]();
  const feedback = document.createElement('p');
  feedback.className = 'practice-feedback';
  feedback.setAttribute('aria-live', 'polite');
  feedback.textContent = lastFeedbackKey ? t(lastFeedbackKey) : '';
  const { controls, nextButton } = buildControls();
  stage.append(roundTitle, instruction, page, feedback, controls);

  root.append(header, stage);

  if (focusTarget === 'title') {
    roundTitle.focus();
  } else if (focusTarget === 'next' && nextButton) {
    nextButton.focus();
  }
}

root.addEventListener('click', (event) => {
  const el = (event.target as HTMLElement).closest<HTMLElement>('.practice-part');
  if (!el || !root.contains(el)) {
    return;
  }
  const target: Target = {
    id: el.dataset.partId ?? '',
    kind: el.dataset.kind === 'flag' ? 'flag' : 'decoy',
    flagId: el.dataset.flagId,
  };
  const wasPlaying = state.phase === 'playing';
  const activeId = el.dataset.partId ?? '';
  const result = click(state, target);
  state = result.state;
  if (result.result === 'fine') {
    lastFeedbackKey = 'practice.feedback.fine';
  } else if (result.result === 'already-spotted') {
    lastFeedbackKey = 'practice.feedback.already';
  } else {
    lastFeedbackKey = null;
  }
  let focusTarget: 'title' | 'next' | 'none' = 'none';
  if (wasPlaying && state.phase === 'revealed') {
    focusTarget = 'next'; // third flag found: move focus to "Next round"
  }
  render(focusTarget);
  if (focusTarget === 'none' && activeId) {
    // Keep keyboard focus on the part the person just activated.
    root.querySelector<HTMLElement>(`[data-part-id="${activeId}"]`)?.focus();
  }
});

render('title');
void followSettingsLocale(render).catch((error: unknown) => console.error('practice: language:', error));
