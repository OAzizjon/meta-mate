"""`mm probe chat` (loaded by tools/mm_probe.py like mm_probe_safety/mm_probe_point).

The app's REAL chat request for prompt tuning: the body is built by mm-core's
chat_request through the chat_request example (crates/mm-core/examples/chat_request.rs,
the same print-the-JSON-body pattern as the safety probe's safety_request), sent to the
Meta API with `mm probe stream`'s key handling, budget guard and ledger (the key is never
printed), and the answer is shown raw, then parsed by the app's own matescript parser
(the example's --parse mode) into sentences, <show>, <open>, moods and sounds, with
timings and tokens. --repeat N runs the question N times and prints a compact table
(sentence count, has <show>, name used, length) instead of N raw answers. --file
QUESTIONS.jsonl runs a batch: every {id, lang, q} line is asked on each --models model
with up to --jobs parallel workers; --out gets one JSON line per answer (raw MateScript,
first token ms, total ms, tokens, error), progress prints every 20 answers, and the run
ends with a per model x language table (answered, errors, median and p90 first token).
The --budget/--max-unpriced ledger caps guard the batch like any other probe.
"""

import argparse
import concurrent.futures
import http.client
import json
import math
import statistics
import subprocess
import time
from pathlib import Path

import mm_probe
from mm_probe import ROOT, ProbeError

SPARK = 'muse-spark-1.3'
WONDER = 'muse-wonder-1.0'
DEFAULT_MODELS = (SPARK, WONDER)
BATCH_PROGRESS_EVERY = 20
EXAMPLE = ['cargo', 'run', '-q', '-p', 'mm-core', '--example', 'chat_request', '--']


def split_models(text: str) -> list[str]:
    """--models is a comma-separated list; each entry must be one of the two Muse chat
    models. argparse applies this to the default string too, so args.models is always a list."""
    models = [part.strip() for part in text.split(',') if part.strip()]
    if not models:
        raise argparse.ArgumentTypeError('empty --models list')
    bad = [model for model in models if model not in (SPARK, WONDER)]
    if bad:
        raise argparse.ArgumentTypeError(
            f'unknown chat model(s): {", ".join(bad)} (choose from {SPARK}, {WONDER})')
    return models


def _add_common(parser: argparse.ArgumentParser) -> None:
    """The budget-capped probe flags, shared by the subcommand and the standalone parser."""
    parser.add_argument('--provider', choices=sorted(mm_probe.PROVIDERS), default='meta')
    parser.add_argument('--model', choices=(SPARK, WONDER), help='default: muse-spark-1.3 (the app default)')
    parser.add_argument('--timeout', type=float, default=120)
    parser.add_argument('--budget', type=float, default=mm_probe.DEFAULT_BUDGET_USD,
                        help='USD cap for this provider in the ledger')
    parser.add_argument('--max-unpriced', type=int, default=30,
                        help='cap on calls of unknown price for this provider')
    parser.add_argument('--price', type=float, nargs=2, metavar=('IN', 'OUT'), help='USD per 1M input/output tokens')
    parser.add_argument('--reasoning', metavar='EFFORT', help='reasoning effort sent to the API')


def add_chat_arguments(sp: argparse.ArgumentParser) -> None:
    """Every chat flag, single-question and --file batch; shared by add_parser and the
    standalone parser so the tests see exactly what the CLI parses."""
    sp.add_argument('question', nargs='?', help='one question; omit it in a --file batch')
    sp.add_argument('--lang', choices=('en', 'ru'), help='language hint, as speech recognition would pass')
    sp.add_argument('--name', help="first name for the request's THE PERSON section")
    sp.add_argument('--about', help='one line about the person for the THE PERSON section')
    sp.add_argument('--repeat', type=int, default=1, metavar='N',
                    help='run the question N times and print a compact table')
    sp.add_argument('--file', metavar='QUESTIONS.jsonl',
                    help='batch mode: ask every {id, lang, q} line on each --models model')
    sp.add_argument('--models', type=split_models, default=','.join(DEFAULT_MODELS), metavar='a,b',
                    help='comma-separated models for --file (default: both Muse chat models)')
    sp.add_argument('--jobs', type=int, default=4, metavar='N', help='parallel --file workers (default: 4)')
    sp.add_argument('--out', metavar='FILE.jsonl',
                    help='the --file answers, one JSON line each (default: QUESTIONS.answers.jsonl)')


def add_parser(sub) -> None:
    """The chat subcommand, with the same budget caps as the other probes."""
    common = argparse.ArgumentParser(add_help=False)
    _add_common(common)
    sp = sub.add_parser('chat', parents=[common],
                        help='the real chat request: persona prompt, THE PERSON with --name/--about, parsed items; '
                             '--file runs a whole JSONL batch')
    add_chat_arguments(sp)
    sp.set_defaults(func=cmd_chat)


def build_chat_parser() -> argparse.ArgumentParser:
    """The chat parser on its own (same flags as the subcommand), for tests and direct runs."""
    parser = argparse.ArgumentParser(prog='mm probe chat')
    _add_common(parser)
    add_chat_arguments(parser)
    parser.set_defaults(func=cmd_chat)
    return parser


def parse_chat_args(argv: list[str]):
    """Parse chat argv with build_chat_parser; used by the tests of the pure CLI parts."""
    return build_chat_parser().parse_args(argv)


def _example_json(tail: list[str]):
    """Run the chat_request example and parse the single JSON value it prints to stdout."""
    cmd = [*EXAMPLE, *tail]
    try:
        proc = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, encoding='utf-8', errors='replace',
                              timeout=600, stdin=subprocess.DEVNULL)
    except subprocess.TimeoutExpired:
        raise ProbeError('cargo timed out building the chat_request example')
    if proc.returncode != 0:
        raise ProbeError(f'chat_request example failed: {(proc.stderr or proc.stdout).strip()[:400]}')
    try:
        return json.loads(proc.stdout)
    except ValueError:
        raise ProbeError(f'chat_request example printed no JSON: {proc.stdout.strip()[:200]!r}')


def chat_request_body(args) -> dict:
    """The real request body: mm-core's chat_request (persona prompt) plus the THE PERSON
    section when --name/--about are given, exactly as the app builds it."""
    tail = ['--question', args.question]
    for flag, value in (('--lang', args.lang), ('--name', args.name), ('--about', args.about),
                        ('--model', args.model), ('--reasoning', args.reasoning)):
        if value:
            tail += [flag, value]
    body = _example_json(tail)
    if not isinstance(body, dict) or not isinstance(body.get('input'), list):
        raise ProbeError('the chat_request example did not print a request body')
    return body


def matescript_items(answer: str) -> list:
    """The app's own parser (the example's --parse mode): the sentences, <show>, <open>,
    moods and sounds of the raw MateScript answer."""
    items = _example_json(['--parse', answer])
    if not isinstance(items, list):
        raise ProbeError('the matescript parser did not print a list of items')
    return items


def kind_of(item) -> str:
    """The parser's items serialize as one-key maps: {"sentence": ...}, {"show": ...}, ..."""
    if isinstance(item, dict) and len(item) == 1:
        return next(iter(item))
    return '?'


def describe_items(items: list) -> list[str]:
    lines = []
    for item in items:
        kind = kind_of(item)
        shown = item if kind == '?' else item[kind]
        lines.append(f'  {kind}: {shown}')
    return lines or ['  (no items)']


def format_table(rows: list[dict]) -> str:
    """The --repeat summary: sentence count, has <show>, the name used, answer length."""
    lines = [f'{"run":>3}  {"sentences":>9}  {"<show>":>6}  {"name":>12}  {"chars":>6}']
    for run, row in enumerate(rows, 1):
        show = 'yes' if row['show'] else '-'
        lines.append(f'{run:>3}  {row["sentences"]:>9}  {show:>6}  {row["name"]:>12}  {row["chars"]:>6}')
    return '\n'.join(lines)


def read_questions(path: str) -> list[dict]:
    """The --file input: one JSON object per non-blank line, each with id, lang and q.
    The line number goes into every error message; a bad line fails the batch before any
    question is asked (and paid for)."""
    try:
        text = Path(path).read_text(encoding='utf-8')
    except OSError as error:
        raise ProbeError(f'{path}: cannot read it ({type(error).__name__})') from None
    questions = []
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        try:
            item = json.loads(line)
        except ValueError as error:
            raise ProbeError(f'{path}: line {number} is not JSON ({error})') from None
        if not isinstance(item, dict):
            raise ProbeError(f'{path}: line {number} is not a JSON object')
        missing = [key for key in ('id', 'lang', 'q') if key not in item]
        if missing:
            raise ProbeError(f'{path}: line {number} misses {", ".join(missing)}')
        if item['lang'] not in ('en', 'ru'):
            raise ProbeError(f'{path}: line {number}: lang must be en or ru, got {item["lang"]!r}')
        questions.append(item)
    return questions


def _median(values: list[int]) -> float | None:
    return statistics.median(values) if values else None


def _p90(values: list[int]) -> int | None:
    """Nearest-rank p90: the value at rank ceil(0.9 * n) of the sorted list."""
    if not values:
        return None
    return values[max(0, math.ceil(0.9 * len(values)) - 1)]


def summarize(records: list[dict]) -> list[dict]:
    """The batch summary: one row per model x language, with the answer count, the error
    count, and the median/p90 time to first token of the answers that streamed (a group
    with only errors has no timings)."""
    groups: dict[tuple[str, str], list[dict]] = {}
    for record in records:
        groups.setdefault((record['model'], record['lang']), []).append(record)
    rows = []
    for (model, lang), items in sorted(groups.items()):
        errors = sum(1 for item in items if item.get('error'))
        firsts = sorted(item['first_ms'] for item in items
                        if not item.get('error') and item.get('first_ms') is not None)
        rows.append({'model': model, 'lang': lang, 'answered': len(items) - errors, 'errors': errors,
                     'median_ms': _median(firsts), 'p90_ms': _p90(firsts)})
    return rows


def format_summary(rows: list[dict]) -> str:
    """The batch's final table: answered, errors, median and p90 first token per group."""
    lines = [f'{"model":<18} {"lang":<4} {"answered":>8} {"errors":>6} {"median first":>12} {"p90 first":>10}']
    for row in rows:
        median = '-' if row['median_ms'] is None else f'{row["median_ms"]:.0f}'
        p90 = '-' if row['p90_ms'] is None else f'{row["p90_ms"]:.0f}'
        lines.append(f'{row["model"]:<18} {row["lang"]:<4} {row["answered"]:>8} {row["errors"]:>6} '
                     f'{median:>12} {p90:>10}')
    return '\n'.join(lines)


def _stream_collect(base: str, key: str, payload: dict, timeout: float):
    """`mm probe stream`'s SSE loop, quiet: the answer is printed once, after parsing, so
    --repeat can tabulate instead of echoing N raw answers."""
    started = time.perf_counter()
    first, text, response, events, completed, failure = None, [], {}, [], False, None
    try:
        with mm_probe._request(base + '/responses', key, payload, timeout, stream=True) as resp:
            for event, data in mm_probe.iter_sse(resp):
                if data.strip() == '[DONE]':
                    completed = completed or (bool(text) and failure is None)
                    break
                try:
                    d = json.loads(data)
                except ValueError:
                    continue
                kind = d.get('type') or event or ''
                events.append(kind)
                if kind.endswith('output_text.delta') and isinstance(d.get('delta'), str):
                    if first is None:
                        first = time.perf_counter() - started
                    text.append(d['delta'])
                elif kind.endswith('output_text.done') and not text and isinstance(d.get('text'), str):
                    text.append(d['text'])
                elif kind in ('response.completed', 'response.done') and isinstance(d.get('response'), dict):
                    response = d['response']
                    completed = response.get('status') in (None, 'completed') and not response.get('error')
                    if not completed:
                        failure = f'status {response.get("status")}'
                elif kind in ('error', 'response.failed', 'response.incomplete') or d.get('error'):
                    if isinstance(d.get('response'), dict):
                        response = d['response']  # its usage still goes to the ledger
                    failure = mm_probe.scrub(json.dumps(d, ensure_ascii=False), key)[:400]
                    break
    except (OSError, http.client.HTTPException) as error:
        failure = f'connection lost mid-stream ({type(error).__name__})'
    return first, ''.join(text), response, events, completed, failure, time.perf_counter() - started


def cmd_chat(args) -> int:
    """The app's chat path end to end: real request body, streamed answer, parsed items,
    timings and tokens; --repeat N tabulates instead; --file runs a JSONL batch."""
    if getattr(args, 'file', None):
        return cmd_chat_file(args)
    if not args.question:
        raise ProbeError('pass a QUESTION, or --file QUESTIONS.jsonl for a batch')
    if args.provider != 'meta':
        raise ProbeError("chat sends the app's Muse chat models: --provider meta only")
    if args.repeat < 1:
        raise ProbeError('--repeat must be at least 1')
    _, key, base = mm_probe.provider_config('meta')
    mm_probe.guard_budget('meta', args.budget, args.max_unpriced)
    payload = chat_request_body(args)
    model = args.model or SPARK
    rows = []
    for run in range(1, args.repeat + 1):
        if args.repeat > 1:
            print(f'--- run {run}/{args.repeat}', flush=True)
        first, answer, response, events, completed, failure, total = _stream_collect(base, key, payload, args.timeout)
        tokens = mm_probe.usage_tokens(response)
        usd = mm_probe.cost_usd(model, *tokens, args.price, (response.get('usage') or {}).get('cost'))
        # The call is paid once the answer is here: the ledger row comes before anything that can fail.
        mm_probe.ledger_add({'ts': time.time(), 'provider': 'meta', 'model': model, 'kind': 'chat',
                             'in': tokens[0], 'out': tokens[1], 'usd': usd})
        if failure or not completed:
            raise ProbeError(f'run {run}: stream not completed: {failure or "no response.completed event"}')
        items = matescript_items(answer)
        if args.repeat == 1:
            print(f'raw answer:\n{mm_probe.scrub(answer, key) or "(empty)"}')
            print('\nparsed items (mm-core matescript):')
            for line in describe_items(items):
                print(line)
            print()
            mm_probe.report(model, first, total, tokens, usd)
        rows.append({'sentences': sum(1 for item in items if kind_of(item) == 'sentence'),
                     'show': any(kind_of(item) == 'show' for item in items),
                     'name': args.name or '-', 'chars': len(answer)})
        path = mm_probe.record('chat', {'provider': 'meta', 'model': model, 'kind': 'chat',
                                        'question': args.question, 'lang': args.lang, 'name': args.name,
                                        'about': args.about, 'total_ms': round(total * 1000),
                                        'first_token_ms': None if first is None else round(first * 1000),
                                        'text': answer, 'items': items,
                                        'event_types': sorted(set(events)), 'response': response}, key)
        if args.repeat == 1:
            print(f'recorded: {path.relative_to(ROOT).as_posix()}')
    if args.repeat > 1:
        print()
        print(format_table(rows))
    return 0


def ask_one(item: dict, model: str, args, base: str, key: str) -> tuple[dict, dict | None]:
    """One question on one model, quietly: the real request body (chat_request_body) and
    the app's own stream collection (_stream_collect), nothing printed. Returns the
    JSONL-ready answer row and the ledger row (None when no response ever arrived, so
    nothing is paid)."""
    job = argparse.Namespace(question=item['q'], lang=item['lang'], name=args.name, about=args.about,
                             model=model, reasoning=args.reasoning)
    started = time.perf_counter()
    try:
        payload = chat_request_body(job)
        first, answer, response, events, completed, failure, total = _stream_collect(base, key, payload, args.timeout)
        tokens = mm_probe.usage_tokens(response)
        usd = mm_probe.cost_usd(model, *tokens, args.price, (response.get('usage') or {}).get('cost'))
        ledger = {'ts': time.time(), 'provider': 'meta', 'model': model, 'kind': 'chat',
                  'in': tokens[0], 'out': tokens[1], 'usd': usd}
        error = '' if completed and not failure else (failure or 'no response.completed event')
    except Exception as exc:  # one bad question must not stop the whole batch
        first, answer, total, tokens = None, '', time.perf_counter() - started, (0, 0)
        ledger = None
        error = f'{type(exc).__name__}: {exc}'[:400]
    return {'id': item['id'], 'lang': item['lang'], 'model': model, 'q': item['q'], 'answer': answer,
            'first_ms': None if first is None else round(first * 1000),
            'total_ms': round(total * 1000),
            'tokens_in': tokens[0], 'tokens_out': tokens[1], 'error': error}, ledger


def cmd_chat_file(args) -> int:
    """The --file batch: every question on every --models model, up to --jobs parallel
    workers, one JSON line per answer in --out, progress every 20 answers, then the per
    model x language summary. The budget is guarded once up front, like any other probe;
    each answer is ledgered as it lands (see ask_one)."""
    if args.provider != 'meta':
        raise ProbeError("chat sends the app's Muse chat models: --provider meta only")
    if args.question:
        raise ProbeError('pass either QUESTION or --file, not both')
    if args.repeat != 1:
        raise ProbeError('--repeat is for one question; --file asks each question once per model')
    if args.jobs < 1:
        raise ProbeError('--jobs must be at least 1')
    questions = read_questions(args.file)
    if not questions:
        raise ProbeError(f'{args.file}: no questions')
    out = Path(args.out) if args.out else Path(args.file).with_suffix('.answers.jsonl')
    _, key, base = mm_probe.provider_config('meta')
    mm_probe.guard_budget('meta', args.budget, args.max_unpriced)
    jobs = [(item, model) for item in questions for model in args.models]
    records: list[dict] = []
    print(f'{len(questions)} question(s) x {len(args.models)} model(s) = {len(jobs)} answers, '
          f'{args.jobs} worker(s) -> {out}')
    with out.open('w', encoding='utf-8') as sink, \
            concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = [pool.submit(ask_one, item, model, args, base, key) for item, model in jobs]
        for done, future in enumerate(concurrent.futures.as_completed(futures), 1):
            record, ledger = future.result()
            if ledger is not None:
                # The call is paid once the answer is here: the ledger row comes before anything that can fail.
                mm_probe.ledger_add(ledger)
            sink.write(json.dumps(record, ensure_ascii=False) + '\n')
            sink.flush()
            records.append(record)
            if done % BATCH_PROGRESS_EVERY == 0 or done == len(jobs):
                print(f'{done} of {len(jobs)}', flush=True)
    print()
    print(format_summary(summarize(records)))
    print(f'wrote {len(records)} answers to {out}')
    return 0
