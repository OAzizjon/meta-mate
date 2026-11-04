"""Assemble the demo with the standard library and ffmpeg only."""

import argparse
import json
import math
from pathlib import Path, PureWindowsPath
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
STILLS = {'.png', '.jpg', '.jpeg', '.bmp', '.webp'}
# A laptop microphone records speech some 30 dB too quiet: clean it, then bring it to level.
VOICE_CLEAN = 'highpass=f=90,afftdn=nf=-30,loudnorm=I=-16:TP=-1.5:LRA=9,aresample=48000'
LEVEL = 'loudnorm=I=-16:TP=-1.5:LRA=11,aresample=48000'
CAMERA_LOOK = 'eq=gamma=1.12:contrast=1.04:saturation=1.05'
BURN_NAME = 'final-burn.srt'
BURN_STYLE = ('FontName=Arial,FontSize=12,Bold=1,PrimaryColour=&H00FFFFFF,BackColour=&H99000000,'
              'BorderStyle=3,Outline=1,Shadow=0,MarginV=18')


def media_path(base, name):
    if not isinstance(name, str) or not name:
        raise ValueError('Media paths must be relative')
    relative = PureWindowsPath(name)
    if relative.anchor or '..' in relative.parts:
        raise ValueError('Media path escapes ' + base.name)
    return base.joinpath(*relative.parts)


def load_list(text):
    """Parse JSON without filesystem or process access."""
    rows = json.loads(text)
    if not isinstance(rows, list) or not rows:
        raise ValueError('Cut list must be a nonempty list')
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get('id'), str) or not row['id']:
            raise ValueError('Every segment needs an id')
        media_path(Path('video'), row.get('src'))
        if 'voice' in row:
            media_path(Path('video/voice'), row['voice'])
        if 'keep_audio' in row and not isinstance(row['keep_audio'], bool):
            raise ValueError(row['id'] + ': keep_audio must be boolean')
        for key in ('in', 'out', 'seconds'):
            if key in row:
                number(row[key], row['id'] + ': ' + key)
        if row.get('out', math.inf) <= row.get('in', 0):
            raise ValueError(row['id'] + ': require in < out')
        if 'seconds' in row and row['seconds'] <= 0:
            raise ValueError(row['id'] + ': seconds must be positive')
        if 'seconds' in row and 'out' in row:
            raise ValueError(row['id'] + ': use either out or seconds, not both')
        if 'crop' in row:
            crop = row['crop']
            if not isinstance(crop, list) or len(crop) != 4:
                raise ValueError(row['id'] + ': crop must be [width, height, x, y]')
            for part in crop:
                number(part, row['id'] + ': crop')
        if 'subtitle' in row and not isinstance(row['subtitle'], str):
            raise ValueError(row['id'] + ': subtitle must be text')
        if not isinstance(row.get('captions', []), list):
            raise ValueError(row['id'] + ': captions must be a list')
        for cap in row.get('captions', []):
            if not isinstance(cap, dict) or not isinstance(cap.get('text'), str):
                raise ValueError(row['id'] + ': caption needs text')
            number(cap.get('at'), 'caption at')
            number(cap.get('for'), 'caption for')
            if cap['for'] <= 0:
                raise ValueError('caption for must be positive')
    return rows


def resolve_sources(rows, video):
    """OBS records .mkv by default and a phone .m4a: takes/A.mp4 may be there as takes/A.mkv."""
    for row in rows:
        if not media_path(video, row['src']).is_file():
            for suffix in ('.mkv', '.mov', '.webm', '.m4v'):
                other = Path(row['src']).with_suffix(suffix)
                if media_path(video, other.as_posix()).is_file():
                    row['src'] = other.as_posix()
                    break
        if row.get('voice') and not media_path(video / 'voice', row['voice']).is_file():
            for suffix in ('.m4a', '.mp3', '.aac', '.ogg', '.mp4', '.mkv'):
                other = Path(row['voice']).with_suffix(suffix)
                if media_path(video / 'voice', other.as_posix()).is_file():
                    row['voice'] = other.as_posix()
                    break


def number(value, label):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
        raise ValueError(label + ' must be a finite nonnegative number')
    return value


def start_times(rows, metadata=None):
    """Return (row, start, duration), resolving unbounded clips from metadata."""
    metadata = metadata or {}
    result, start = [], 0.0
    for row in rows:
        end = row.get('out', metadata.get(row['src'], {}).get('duration'))
        actual = metadata.get(row['src'], {}).get('duration')
        if actual is not None and row.get('in', 0) >= actual:
            raise ValueError(row['id'] + ': in is past the source duration')
        duration = row.get('seconds')
        if duration is None:
            if end is None:
                raise ValueError(row['id'] + ': specify seconds or out for --dry-run')
            duration = end - row.get('in', 0)
        if duration <= 0:
            raise ValueError(row['id'] + ': empty segment')
        for cap in row.get('captions', []):
            if cap['at'] + cap['for'] > duration + 1e-9:
                raise ValueError(row['id'] + ': caption exceeds segment')
        result.append((row, start, duration))
        start += duration
    return result


def validate(rows, existing, metadata=None):
    """Validate against a supplied set of existing video-relative paths."""
    missing = []
    for row in rows:
        for name in [row['src']] + (['voice/' + row['voice']] if row.get('voice') else []):
            if name not in existing:
                missing.append(row['id'] + ': missing ' + name)
    if missing:
        raise ValueError('\n'.join(missing))
    timeline = start_times(rows, metadata)
    total = sum(item[2] for item in timeline)
    if total > 180.0:
        longest = sorted(timeline, key=lambda item: item[2], reverse=True)[:5]
        raise ValueError(f'Total {total:.3f} s exceeds 180.0 s; longest: ' +
                         ', '.join(f'{r["id"]} {d:.3f}s' for r, _, d in longest))
    return timeline


def warnings(timeline, metadata):
    """What a person should fix in the list, found once the real lengths are known."""
    notes, voices = [], []
    for row, start, duration in timeline:
        actual = metadata.get(row['src'], {}).get('duration')
        # Clips are meant to hold their last frame; a take that does it freezes the speaker.
        if actual is not None and row['src'].startswith('takes/'):
            short = row.get('in', 0) + duration - actual
            if short > 0.05:
                notes.append(f'{row["id"]}: {row["src"]} ends {short:.1f} s before its segment, so its last frame '
                             'holds in silence; lower "seconds" or "in"')
        if row.get('voice'):
            name = 'voice/' + row['voice']
            if metadata.get(name, {}).get('audio') is False:
                notes.append(f'{row["id"]}: {name} has no sound')
            voices.append((start, metadata.get(name, {}).get('duration'), row['id'], name))
    total = sum(item[2] for item in timeline)
    for i, (start, length, row_id, name) in enumerate(voices):
        if length is None:
            continue
        limit = voices[i + 1][0] if i + 1 < len(voices) else total
        if start + length > limit + 0.05:
            notes.append(f'{row_id}: {name} is {length:.1f} s and runs {start + length - limit:.1f} s into what follows; '
                         'record it shorter or lengthen its segment')
    return notes


def drawtext_escape(text):
    """Escape both ffmpeg's filtergraph and drawtext option parsers (unquoted)."""
    text = text.replace('\\', '\\\\').replace("'", "\\'").replace(':', '\\:')
    return ''.join('\\' + char if char in "\\'[],;" else char for char in text)


def look_filters(row):
    """The picture's own corrections, each ending in a comma for the chain."""
    parts = []
    if row.get('crop'):
        width, height, x, y = row['crop']
        parts.append(f'crop={width}:{height}:{x}:{y}')
    if row.get('look') == 'camera':
        parts.append(CAMERA_LOOK)
    return ''.join(part + ',' for part in parts)


def subtitle_lines(timeline, metadata=None):
    """(start, end, sentence) per spoken sentence, timed by its share of the words. A voice
    shorter than its segment is spoken from the segment's start."""
    metadata = metadata or {}
    lines = []
    for row, start, duration in timeline:
        if not row.get('subtitle'):
            continue
        sentences = [part.strip() for part in re.split(r'(?<=[.?!]["”])\s+|(?<=[.?!])\s+', row['subtitle']) if part.strip()]
        words = sum(len(part.split()) for part in sentences)
        if not words:
            continue
        spoken = metadata.get('voice/' + row['voice'], {}).get('duration') if row.get('voice') else None
        span = min(duration, spoken) if spoken else duration
        at = start
        for part in sentences:
            length = span * len(part.split()) / words
            lines.append((at, at + length, part))
            at += length
    return lines


def write_srt(entries):
    def stamp(seconds):
        ms = round(seconds * 1000)
        return f'{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02},{ms % 1000:03}'
    return '\n\n'.join(f'{i}\n{stamp(a)} --> {stamp(b)}\n{text}'
                       for i, (a, b, text) in enumerate(entries, 1)) + ('\n' if entries else '')


def build_graph(timeline, video, draft=False, music=None, metadata=None, burn=False):
    """Return ffmpeg input arguments and one filter graph; no I/O."""
    metadata = metadata or {}
    args, graph, voices = [], [], []
    w, h = (1280, 720) if draft else (1920, 1080)
    index = 0
    for i, (row, start, duration) in enumerate(timeline):
        src = media_path(video, row['src'])
        if src.suffix.lower() in STILLS:
            args += ['-loop', '1']
        args += ['-i', str(src)]
        trim = f'trim=start={row.get("in", 0)}'
        if 'out' in row:
            trim += f':end={row["out"]}'
        chain = (f'[{index}:v]{trim},setpts=PTS-STARTPTS,{look_filters(row)}scale={w}:{h}:force_original_aspect_ratio=decrease,'
                 f'pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30,'
                 f'tpad=stop_mode=clone:stop_duration={duration},trim=duration={duration},setpts=PTS-STARTPTS')
        for cap in row.get('captions', []):
            font = drawtext_escape('C:/Windows/Fonts/arialbd.ttf')
            chain += (f',drawtext=fontfile={font}:text={drawtext_escape(cap["text"])}:expansion=none:'
                      f'fontsize={32 if draft else 48}:fontcolor=white:box=1:boxcolor=black@0.7:boxborderw=12:'
                      f'x=(w-text_w)/2:y=h*0.72:enable=gte(t,{cap["at"]})*lt(t,{cap["at"] + cap["for"]})')
        # Commas in enable expressions belong to the expression, not the graph.
        chain = chain.replace('gte(t,', 'gte(t\\,').replace(')*lt(t,', ')*lt(t\\,')
        graph.append(chain + f'[v{i}]')
        own = row.get('keep_audio', row['src'].replace('\\', '/').startswith('takes/'))
        own = own and metadata.get(row['src'], {}).get('audio', True)
        if own:
            audio = f'[{index}:a]atrim=start={row.get("in", 0)}'
            if 'out' in row:
                audio += f':end={row["out"]}'
            clean = VOICE_CLEAN if row.get('look') == 'camera' else LEVEL
            audio += f',asetpts=PTS-STARTPTS,aresample=48000:async=1:first_pts=0,{clean},aformat=channel_layouts=stereo,apad'
        else:
            audio = 'anullsrc=r=48000:cl=stereo'
        graph.append(audio + f',atrim=duration={duration}[a{i}]')
        index += 1
        if row.get('voice'):
            name = 'voice/' + row['voice']
            args += ['-i', str(media_path(video, name))]
            # Narration may span adjacent pictures in the shoot table.
            length = metadata.get(name, {}).get('duration') or duration
            voices.append((index, start, length))
            index += 1
    video_out = '[vjoined]' if burn else '[vout]'
    graph.append(''.join(f'[v{i}][a{i}]' for i in range(len(timeline))) +
                 f'concat=n={len(timeline)}:v=1:a=1{video_out}[base]')
    if burn:
        # ffmpeg runs in the output folder, so the name needs no Windows path escaping.
        graph.append(f"[vjoined]subtitles={BURN_NAME}:force_style='{BURN_STYLE}'[vout]")
    total = sum(d for _, _, d in timeline)
    duck = '+'.join(f'between(t,{s},{s + d})' for _, s, d in voices)
    if duck:
        graph.append(f"[base]volume='if(gt({duck},0),0.316227766,1)':eval=frame[bed]")
    else:
        graph.append('[base]volume=1[bed]')
    tracks = ['[bed]']
    for n, (idx, start, _) in enumerate(voices):
        graph.append(f'[{idx}:a]asetpts=PTS-STARTPTS,{VOICE_CLEAN},aformat=channel_layouts=stereo,'
                     f'adelay={round(start * 1000)}:all=1[voice{n}]')
        tracks.append(f'[voice{n}]')
    if music:
        args += ['-stream_loop', '-1', '-i', str(music)]
        graph.append(f'[{index}:a]aresample=48000,aformat=channel_layouts=stereo,asetpts=PTS-STARTPTS,'
                     f'volume=0.063095734,atrim=duration={total},afade=t=out:st={max(0, total - 3)}:d=3[music]')
        tracks.append('[music]')
    graph.append(''.join(tracks) + f'amix=inputs={len(tracks)}:duration=first:normalize=0,alimiter=limit=1:level=0[mixed]')
    return args, ';\n'.join(graph)


def srt(timeline):
    def stamp(seconds):
        ms = round(seconds * 1000)
        return f'{ms // 3600000:02}:{ms // 60000 % 60:02}:{ms // 1000 % 60:02},{ms % 1000:03}'
    entries = []
    for row, start, duration in timeline:
        if row.get('subtitle'):
            entries.append((start, start + duration, row['subtitle']))
        for cap in row.get('captions', []):
            entries.append((start + cap['at'], start + cap['at'] + cap['for'], cap['text']))
    return '\n\n'.join(f'{i}\n{stamp(a)} --> {stamp(b)}\n{text}'
                       for i, (a, b, text) in enumerate(sorted(entries), 1)) + ('\n' if entries else '')


def probe(path):
    result = subprocess.run(['ffmpeg', '-hide_banner', '-i', str(path)], capture_output=True, text=True,
                            encoding='utf-8', errors='replace', timeout=30)
    match = re.search(r'Duration: (\d+):(\d+):(\d+(?:\.\d+)?)', result.stderr)
    duration = sum(float(v) * factor for v, factor in zip(match.groups(), (3600, 60, 1))) if match else None
    return {'duration': duration, 'audio': bool(re.search(r'Stream .*Audio:', result.stderr))}


def main(argv=None, root=ROOT):
    parser = argparse.ArgumentParser(prog='mm cut', description=__doc__)
    parser.add_argument('--draft', action='store_true')
    parser.add_argument('--dry-run', action='store_true')
    parser.add_argument('--list', type=Path, default=root / 'video/cut.json')
    parser.add_argument('--music', type=Path)
    parser.add_argument('--no-burn', action='store_true', help='leave the subtitles out of the picture')
    args = parser.parse_args(argv)
    video = root / 'video'
    try:
        rows = load_list(args.list.read_text(encoding='utf-8'))
        resolve_sources(rows, video)
        names = {r['src'] for r in rows} | {'voice/' + r['voice'] for r in rows if r.get('voice')}
        for name in names:
            base = video / 'voice' if name.startswith('voice/') else video
            if not media_path(video, name).resolve().is_relative_to(base.resolve()):
                raise ValueError('Media path escapes video: ' + name)
        existing = {n for n in names if media_path(video, n).is_file()}
        # Missing media is reported before any subprocess is started.
        if names != existing:
            validate(rows, existing)
        if args.music and not args.music.is_file():
            raise ValueError('Missing music: ' + str(args.music))
        if all('seconds' in r or 'out' in r for r in rows):
            validate(rows, existing)
        metadata = {} if args.dry_run else {n: probe(media_path(video, n)) for n in names}
        timeline = validate(rows, existing, metadata)
        print('ID                 Start      Seconds  Source')
        for row, start, duration in timeline:
            print(f'{row["id"]:18} {start:8.3f} {duration:8.3f}  {row["src"]}')
        total = sum(d for _, _, d in timeline)
        print(f'Total: {total:.3f} s')
        for note in warnings(timeline, metadata):
            print('Warning: ' + note)
        lines = subtitle_lines(timeline, metadata)
        burn = bool(lines) and not args.no_burn
        music = args.music.resolve() if args.music else None
        inputs, graph = build_graph(timeline, video, args.draft, music, metadata, burn)
        out = video / 'out'
        script = out / 'final.filter.txt'
        command = ['ffmpeg', '-nostdin', '-y', *inputs, '-/filter_complex', str(script),
                   '-map', '[vout]', '-map', '[mixed]', '-t', str(total), '-c:v', 'libx264',
                   '-preset', 'ultrafast' if args.draft else 'medium', '-crf', '20', '-pix_fmt', 'yuv420p',
                   '-c:a', 'aac', '-b:a', '192k', '-movflags', '+faststart', str(out / 'final.mp4')]
        print(subprocess.list2cmdline(command))
        if args.dry_run:
            print('Dry run: no files written. Audio presence and voice lengths are not probed.')
            return 0
        out.mkdir(parents=True, exist_ok=True)
        script.write_text(graph, encoding='utf-8')
        (out / 'final.srt').write_text(srt(timeline), encoding='utf-8')
        (out / BURN_NAME).write_text(write_srt(lines), encoding='utf-8')
        return subprocess.call(command, cwd=out)
    except (OSError, ValueError, TypeError, subprocess.TimeoutExpired) as error:
        print('mm cut: ' + str(error))
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
