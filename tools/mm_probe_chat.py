"""`mm probe chat` (loaded by tools/mm_probe.py like mm_probe_safety/mm_probe_point).

The app's REAL chat request for prompt tuning: the body is built by mm-core's
chat_request through the chat_request example (crates/mm-core/examples/chat_request.rs,
the same print-the-JSON-body pattern as the safety probe's safety_request), sent to the
Meta API with `mm probe stream`'s key handling, budget guard and ledger (the key is never
printed), and the answer is shown raw, then parsed by the app's own matescript parser
(the example's --parse mode) into sentences, <show>, <open>, moods and sounds, with
timings and tokens. --repeat N runs the question N times and prints a compact table
(sentence count, has <show>, name used, length) instead of N raw answers.
"""

import argparse
import http.client
import json
import subprocess
import time

import mm_probe
from mm_probe import ROOT, ProbeError

SPARK = 'muse-spark-1.3'
WONDER = 'muse-wonder-1.0'
EXAMPLE = ['cargo', 'run', '-q', '-p', 'mm-core', '--example', 'chat_request', '--']


def add_parser(sub) -> None:
    """The chat subcommand, with the same budget caps as the other probes."""
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument('--provider', choices=sorted(mm_probe.PROVIDERS), default='meta')
    common.add_argument('--model', choices=(SPARK, WONDER), help='default: muse-spark-1.3 (the app default)')
    common.add_argument('--timeout', type=float, default=120)
    common.add_argument('--budget', type=float, default=mm_probe.DEFAULT_BUDGET_USD,
                        help='USD cap for this provider in the ledger')
    common.add_argument('--max-unpriced', type=int, default=30,
                        help='cap on calls of unknown price for this provider')
    common.add_argument('--price', type=float, nargs=2, metavar=('IN', 'OUT'), help='USD per 1M input/output tokens')
    common.add_argument('--reasoning', metavar='EFFORT', help='reasoning effort sent to the API')
    sp = sub.add_parser('chat', parents=[common],
                        help='the real chat request: persona prompt, THE PERSON with --name/--about, parsed items')
    sp.add_argument('question')
    sp.add_argument('--lang', choices=('en', 'ru'), help='language hint, as speech recognition would pass')
    sp.add_argument('--name', help="first name for the request's THE PERSON section")
    sp.add_argument('--about', help='one line about the person for the THE PERSON section')
    sp.add_argument('--repeat', type=int, default=1, metavar='N',
                    help='run the question N times and print a compact table')
    sp.set_defaults(func=cmd_chat)


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
    timings and tokens; --repeat N tabulates instead."""
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
