import assert from 'node:assert/strict';
import { join } from 'node:path';
import { test } from 'node:test';

import { devtoolsUrlFrom, ffmpegArgs, frameTimes, parseClipMeta } from '../render_clips.mjs';

const page = (content) => `<!doctype html><html><head><meta name="clip" content="${content}"></head><body></body></html>`;

test('parseClipMeta parses the full example meta', () => {
  const parsed = parseClipMeta(page('duration=4.0;fps=60;width=1920;height=1080;alpha=0'));
  assert.deepEqual(parsed, { duration: 4, fps: 60, width: 1920, height: 1080, alpha: 0 });
});

test('parseClipMeta applies documented defaults', () => {
  assert.deepEqual(parseClipMeta(page('duration=2.5')), { duration: 2.5, fps: 60, width: 1920, height: 1080, alpha: 0 });
});

test('parseClipMeta tolerates whitespace around keys and values', () => {
  const parsed = parseClipMeta(page(' duration = 1.5 ; fps = 24 ; width = 640 ; height = 360 ; alpha = 1 '));
  assert.deepEqual(parsed, { duration: 1.5, fps: 24, width: 640, height: 360, alpha: 1 });
});

test('parseClipMeta finds the clip meta regardless of attribute order', () => {
  const html = '<meta content="duration=1.0" name="clip">';
  assert.equal(parseClipMeta(html).duration, 1);
});

test('parseClipMeta throws when the meta tag is missing', () => {
  assert.throws(() => parseClipMeta('<html><head></head></html>'), /clip meta/);
});

test('parseClipMeta reports a missing duration', () => {
  assert.throws(() => parseClipMeta(page('fps=30')), /"duration"/);
});

test('parseClipMeta rejects a non-numeric duration', () => {
  assert.throws(() => parseClipMeta(page('duration=abc')), /"duration"/);
});

test('parseClipMeta rejects an out-of-range duration', () => {
  assert.throws(() => parseClipMeta(page('duration=61')), /"duration"/);
  assert.throws(() => parseClipMeta(page('duration=0.05')), /"duration"/);
});

test('parseClipMeta rejects an out-of-range fps', () => {
  assert.throws(() => parseClipMeta(page('duration=1;fps=0')), /"fps"/);
  assert.throws(() => parseClipMeta(page('duration=1;fps=121')), /"fps"/);
});

test('parseClipMeta rejects an odd width', () => {
  assert.throws(() => parseClipMeta(page('duration=1;width=641')), /"width"/);
});

test('parseClipMeta rejects a too-small width', () => {
  assert.throws(() => parseClipMeta(page('duration=1;width=8')), /"width"/);
});

test('parseClipMeta rejects an odd height', () => {
  assert.throws(() => parseClipMeta(page('duration=1;height=361')), /"height"/);
});

test('parseClipMeta rejects alpha other than 0 or 1', () => {
  assert.throws(() => parseClipMeta(page('duration=1;alpha=2')), /"alpha"/);
  assert.throws(() => parseClipMeta(page('duration=1;alpha=0.5')), /"alpha"/);
});

test('frameTimes returns k/fps for round(duration*fps) frames', () => {
  const times = frameTimes(1, 30);
  assert.equal(times.length, 30);
  assert.equal(times[0], 0);
  assert.equal(times[10], 10 / 30);
  assert.equal(times[29], 29 / 30);
});

test('frameTimes rounds the frame count', () => {
  assert.equal(frameTimes(0.5, 30).length, 15);
  assert.equal(frameTimes(1, 29.97).length, 30);
  assert.equal(frameTimes(0.1, 60).length, 6);
});

test('ffmpegArgs builds a single mp4 output for alpha=0', () => {
  const sets = ffmpegArgs({ name: 'demo', fps: 30, alpha: 0, out: 'out' });
  assert.equal(sets.length, 1);
  const args = sets[0];
  assert.deepEqual(args.slice(0, 7), ['-y', '-f', 'image2pipe', '-framerate', '30', '-i', '-']);
  assert.ok(args.includes('libx264'));
  assert.ok(args.includes('yuv420p'));
  assert.ok(args.includes('16'));
  assert.ok(args.includes('slow'));
  assert.ok(args.includes('+faststart'));
  assert.equal(args.at(-1), join('out', 'demo.mp4'));
});

test('ffmpegArgs builds webm and mov outputs for alpha=1', () => {
  const sets = ffmpegArgs({ name: 'lower', fps: 60, alpha: 1, out: 'out dir' });
  assert.equal(sets.length, 2);
  const [webm, mov] = sets;
  assert.ok(webm.includes('libvpx-vp9'));
  assert.ok(webm.includes('yuva420p'));
  assert.equal(webm.at(-1), join('out dir', 'lower.webm'));
  assert.ok(mov.includes('prores_ks'));
  assert.ok(mov.includes('4444'));
  assert.ok(mov.includes('yuva444p10le'));
  assert.equal(mov.at(-1), join('out dir', 'lower.mov'));
});

test('devtoolsUrlFrom parses the URL from Windows stderr', () => {
  const stderr = 'Some edge log\r\nDevTools listening on ws://127.0.0.1:52341/devtools/browser/abc123\r\n';
  assert.equal(devtoolsUrlFrom(stderr), 'ws://127.0.0.1:52341/devtools/browser/abc123');
});

test('devtoolsUrlFrom returns null when there is no URL', () => {
  assert.equal(devtoolsUrlFrom('nothing here\n'), null);
});
