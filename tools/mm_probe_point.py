"""Pointing benchmark: synthetic fixtures, real app requests and MateScript parsing."""

from collections import Counter
from html.parser import HTMLParser
import base64
import fnmatch
import json
import math
from pathlib import Path
import re
import statistics
import struct
import subprocess
import tempfile
import time

import mm_probe as probe
import mm_probe_safety as safety


class PageParser(HTMLParser):
    """Collect inline declarations, retaining duplicate ids for validation."""
    def __init__(self):
        super().__init__()
        self.elements = {}

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            style = {}
            for declaration in attrs.get('style', '').split(';'):
                if ':' in declaration:
                    key, value = declaration.split(':', 1)
                    style[key.strip().lower()] = value.strip().lower()
            self.elements.setdefault(attrs['id'], []).append(style)


def validate(page):
    parser = PageParser()
    parser.feed(page['path'].read_text(encoding='utf-8-sig'))
    elements = page.get('elements')
    if not isinstance(elements, list) or not elements:
        raise probe.ProbeError('elements must be a nonempty array')
    ids = set()
    for element in elements:
        if not isinstance(element, dict):
            raise probe.ProbeError('element must be an object')
        ident, rect = element.get('id'), element.get('rect')
        if (not isinstance(ident, str) or not re.fullmatch(r'e[0-9]+', ident)
                or ident in ids or not isinstance(element.get('role'), str)
                or not isinstance(element.get('name'), str)):
            raise probe.ProbeError('invalid or duplicate element id/role/name')
        ids.add(ident)
        if (not isinstance(rect, list) or len(rect) != 4
                or any(type(n) is not int for n in rect)):
            raise probe.ProbeError('rect must contain four integers')
        x, y, w, h = rect
        if x < 0 or y < 0 or w <= 0 or h <= 0 or x + w > 1280 or y + h > 800:
            raise probe.ProbeError('rect outside 1280x800')
        styles = parser.elements.get(ident, [])
        expected = dict(position='absolute', left=f'{x}px', top=f'{y}px', width=f'{w}px', height=f'{h}px')
        if len(styles) != 1 or any(styles[0].get(k) != v for k, v in expected.items()):
            raise probe.ProbeError(f'{page["page"]}: {ident} missing or inline rect differs')
    questions = page.get('questions')
    if not isinstance(questions, list) or not questions:
        raise probe.ProbeError('questions must be a nonempty array')
    for question in questions:
        if (not isinstance(question, dict) or not isinstance(question.get('q'), str)
                or not question['q'].strip() or 'target' not in question
                or (question['target'] is not None and
                    (not isinstance(question['target'], str) or question['target'] not in ids))):
            raise probe.ProbeError('question target must name a listed element or be null')


def load_pages(directory, only='*'):
    directory = directory.resolve()
    manifests = sorted(directory.glob('*/manifest.json'))
    if not manifests:
        raise probe.ProbeError('no subfolder manifest.json files')
    pages = []
    for manifest in manifests:
        if not probe.is_plain_file(manifest) or manifest.resolve().parent.parent != directory:
            raise probe.ProbeError('manifest must be a plain file inside DIR')
        rows = json.loads(manifest.read_text(encoding='utf-8-sig'))
        if not isinstance(rows, list):
            raise probe.ProbeError('manifest must be an array')
        seen = set()
        for row in rows:
            if not isinstance(row, dict) or not isinstance(row.get('file'), str):
                raise probe.ProbeError('page requires a file')
            path = manifest.parent / row['file']
            if (path.resolve().parent != manifest.parent.resolve() or path.suffix.lower() != '.html'
                    or not probe.is_plain_file(path) or path.resolve() in seen):
                raise probe.ProbeError('duplicate or invalid page path')
            seen.add(path.resolve())
            if row.get('lang') not in ('en', 'ru') or not isinstance(row.get('app'), str):
                raise probe.ProbeError('page requires lang en/ru and app text')
            page = {**row, 'page': path.relative_to(directory).as_posix(), 'path': path.resolve()}
            validate(page)
            if fnmatch.fnmatchcase(page['page'], only):
                pages.append(page)
    if not pages:
        raise probe.ProbeError('no pages matched')
    return pages


def rust(example, arguments=(), stdin=None):
    result = subprocess.run(['cargo', 'run', '-q', '-p', 'mm-core', '--example', example, '--', *arguments],
                            cwd=probe.ROOT, input=stdin, capture_output=True,
                            text=True, encoding='utf-8', timeout=180)
    if result.returncode:
        raise probe.ProbeError(f'mm-core {example} failed: ' + result.stderr[:500])
    return json.loads(result.stdout)


def prepare(page, model, args):
    png = safety.render_page(page, Path(args.out).resolve() / 'png')
    bodies = []
    # Metadata and rendered pixels are frozen before the shared pool starts.
    with tempfile.TemporaryDirectory(prefix='mm-point-') as folder:
        elements = Path(folder) / 'elements.json'
        elements.write_text(json.dumps(page['elements'], ensure_ascii=False), encoding='utf-8')
        for mode in dict.fromkeys(args.elements.split(',')):
            for question in page['questions']:
                command = ['--image', str(png), '--question', question['q'], '--model', model,
                           '--max-edge', str(args.max_edge), '--elements', str(elements) if mode == 'on' else 'none']
                if args.reasoning:
                    command += ['--reasoning', args.reasoning]
                body = rust('point_request', command)
                # Read the actual encoded dimensions, including resize rounding.
                image = next(part['image_url'] for msg in body['input'] for part in msg['content']
                             if part['type'] == 'input_image')
                size = struct.unpack('>II', base64.b64decode(image.split(',', 1)[1])[16:24])
                bodies.append(({**page, **question, 'elements_mode': mode, 'sent_size': size}, body))
    return bodies


def score(row, point):
    if point is None:
        return 'CORRECT' if row['target'] is None else 'NO-POINT'
    if row['target'] is None:
        return 'FALSE-POINT'
    elements = {e['id']: e['rect'] for e in row['elements']}
    if point['kind'] == 'id':
        return 'HIT' if point['value'] == row['target'] else 'WRONG'
    x, y = point['value']
    width, height = row['sent_size']
    if not (0 <= x < width and 0 <= y < height):
        return 'WRONG'
    x, y = x * 1280 / width, y * 800 / height
    left, top, w, h = elements[row['target']]
    if left <= x < left + w and top <= y < top + h:
        return 'HIT'
    distance = math.hypot(max(left - x, 0, x - left - w), max(top - y, 0, y - top - h))
    return 'NEAR' if distance <= 24 else 'WRONG'


def evaluate(row, answer, final):
    parsed = rust('point_parse', stdin=answer)
    point = parsed[0] if parsed else None
    final['_first_point_ms'] = (next((ms for end, ms in final['_arrivals'] if end >= parsed[1]), None)
                                if parsed else None)
    return dict(point=point, verdict=score(row, point))


def exchange(body, key, base, timeout):
    """Reuse safety's HTTP/SSE path; timestamp complete point tags across deltas."""
    started = time.perf_counter()
    arrivals = []
    length = 0
    def observe(delta):
        nonlocal length
        length += len(delta.encode('utf-8'))
        arrivals.append((length, round((time.perf_counter() - started) * 1000, 3)))
    final, answer = safety.exchange(body, key, base, timeout, observe=observe)
    final['_latency_ms'] = round((time.perf_counter() - started) * 1000, 3)
    if not arrivals and answer:
        arrivals.append((len(answer.encode('utf-8')), final['_latency_ms']))
    final['_arrivals'] = arrivals
    return final, answer


def aggregate(rows):
    groups = []
    for key in sorted({(r['model'], r['elements_mode'], r['lang']) for r in rows}):
        all_rows = [r for r in rows if (r['model'], r['elements_mode'], r['lang']) == key]
        good = [r for r in all_rows if not r.get('error')]
        present = [r for r in good if r['target'] is not None]
        absent = [r for r in good if r['target'] is None]
        counts = Counter(r['verdict'] for r in good)
        group = dict(model=key[0], elements=key[1], lang=key[2], n=len(present), absent_n=len(absent),
                     errors=len(all_rows) - len(good))
        for label in ('HIT', 'NEAR', 'WRONG', 'NO-POINT', 'CORRECT', 'FALSE-POINT'):
            n = len(absent) if label in ('CORRECT', 'FALSE-POINT') else len(present)
            group[label] = counts[label] / n if n else None
        for field in ('first_point_ms', 'latency_ms'):
            times = sorted(r[field] for r in good if r.get(field) is not None)
            group[field + '_n'] = len(times)
            group[field + '_median'] = statistics.median(times) if times else None
            group[field + '_p90'] = times[math.ceil(.9 * len(times)) - 1] if times else None
        group.update(input_tokens=sum(r.get('input_tokens', 0) for r in all_rows),
                     output_tokens=sum(r.get('output_tokens', 0) for r in all_rows),
                     usd=sum(r['usd'] for r in all_rows if r.get('usd') is not None),
                     unpriced=sum('usd' in r and r['usd'] is None for r in all_rows))
        groups.append(group)
    return groups


def write_report(out, rows, errors, dry_run):
    summary = aggregate(rows)
    (out / 'report.json').write_text(json.dumps(dict(dry_run=dry_run, runs=rows, summary=summary, errors=errors),
                                               ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    def cell(value):
        return ('n/a' if value is None else str(round(value, 4) if isinstance(value, float) else value)
                ).replace('|', '\\|').replace('\n', ' ').replace('\r', '')
    lines = ['# Pointing benchmark', '',
             'Rates exclude errors. n counts present targets; absent_n counts absent targets. '
             'CORRECT is the honest no-point rate on absent targets. First-point times measure '
             'completion of the first point tag in streamed text; absent tags are excluded. '
             'p90 uses nearest rank. USD is the sum recorded in the ledger; unpriced calls are separate.', '']
    if summary:
        columns = list(summary[0])
        lines += ['| ' + ' | '.join(columns) + ' |', '|' + '|'.join(['---'] * len(columns)) + '|']
        lines += ['| ' + ' | '.join(cell(g[k]) for k in columns) + ' |' for g in summary]
    lines += ['', '## Most-missed questions (up to 10)', '']
    missed = Counter((r['page'], r['q']) for r in rows if not r.get('error') and r['verdict'] not in ('HIT', 'CORRECT'))
    for (page, question), count in missed.most_common(10):
        lines.append(f'- {cell(page)}: {cell(question)} ({count} misses)')
        for r in rows:
            if (r['page'], r['q']) == (page, question) and 'answer' in r:
                lines.append(f'  - {cell(r["model"])} / {r["elements_mode"]} / {r["repeat"]}: {cell(r["answer"])}')
    lines += ['', '## Errors', '', *['- ' + cell(e) for e in errors]]
    (out / 'report.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')


def cmd_point(args):
    def load(directory, only):
        if any(mode not in ('on', 'off') for mode in args.elements.split(',')):
            raise probe.ProbeError('elements must be on and/or off')
        return load_pages(directory, only)
    return safety.run_batch(args, load, prepare, evaluate, exchange, write_report, 'point')


def add_parser(sub):
    safety.add_parser(sub, 'point', cmd_point)
