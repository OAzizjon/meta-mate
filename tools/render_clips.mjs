#!/usr/bin/env node
// Renders the animated HTML scenes in video/clips/ into frame-exact video clips.
// Drives headless Edge over CDP, seeks every animation to t = k/fps before each
// captured frame, and pipes the PNG frames into ffmpeg. No npm dependencies.

import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { rmSync } from 'node:fs';
import { mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const ROOT = fileURLToPath(new URL('..', import.meta.url));
const CLIPS_DIR = join(ROOT, 'video', 'clips');
const DEFAULT_OUT_DIR = join(ROOT, 'video', 'out');
const DEFAULT_BROWSER = 'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe';
const LAUNCH_TIMEOUT_MS = 30_000;
const CDP_TIMEOUT_MS = 120_000;

const sleep = (ms) => new Promise((resolveSleep) => setTimeout(resolveSleep, ms));

// ---------------------------------------------------------------------------
// Pure helpers, kept exported and browser-free for tools/tests/render_clips.test.mjs
// ---------------------------------------------------------------------------

function metaError(field, value, why) {
  return new Error(`clip meta field "${field}": ${why} (got ${JSON.stringify(value)})`);
}

export function parseClipMeta(html) {
  const tagRe = /<meta\b[^>]*>/gi;
  let tag = null;
  let match;
  while ((match = tagRe.exec(html)) !== null) {
    if (/\bname\s*=\s*(["'])clip\1/i.test(match[0])) {
      tag = match[0];
      break;
    }
  }
  if (tag === null) {
    throw new Error('clip meta: missing <meta name="clip" content="..."> tag');
  }
  const contentMatch = /\bcontent\s*=\s*(["'])([^"']*)\1/i.exec(tag);
  if (!contentMatch) {
    throw new Error('clip meta: <meta name="clip"> tag has no content attribute');
  }

  const raw = {};
  for (const part of contentMatch[2].split(';')) {
    const eq = part.indexOf('=');
    if (eq === -1) continue;
    const key = part.slice(0, eq).trim();
    if (key) raw[key] = part.slice(eq + 1).trim();
  }

  const numberField = (field, { min, max, integer = false } = {}) => {
    if (!(field in raw)) return undefined;
    const text = raw[field];
    const value = Number(text);
    if (!Number.isFinite(value)) throw metaError(field, text, 'expected a number');
    if (integer && !Number.isInteger(value)) throw metaError(field, text, 'expected an integer');
    if (min !== undefined && value < min) throw metaError(field, text, `must be >= ${min}`);
    if (max !== undefined && value > max) throw metaError(field, text, `must be <= ${max}`);
    return value;
  };

  const duration = numberField('duration', { min: 0.1, max: 60 });
  if (duration === undefined) throw metaError('duration', undefined, 'missing (required)');
  const fps = numberField('fps', { min: 1, max: 120 }) ?? 60;
  const width = numberField('width', { min: 16, max: 3840, integer: true }) ?? 1920;
  const height = numberField('height', { min: 16, max: 3840, integer: true }) ?? 1080;
  const alpha = numberField('alpha', { min: 0, max: 1, integer: true }) ?? 0;
  if (width % 2 !== 0) throw metaError('width', raw.width, 'must be an even number');
  if (height % 2 !== 0) throw metaError('height', raw.height, 'must be an even number');

  return { duration, fps, width, height, alpha };
}

export function frameTimes(duration, fps) {
  const count = Math.round(duration * fps);
  return Array.from({ length: count }, (_, k) => k / fps);
}

export function ffmpegArgs({ name, fps, alpha, out }) {
  const input = ['-y', '-f', 'image2pipe', '-framerate', String(fps), '-i', '-'];
  if (alpha === 1) {
    return [
      [...input, '-c:v', 'libvpx-vp9', '-pix_fmt', 'yuva420p', '-b:v', '0', '-crf', '20', join(out, `${name}.webm`)],
      [...input, '-c:v', 'prores_ks', '-profile:v', '4444', '-pix_fmt', 'yuva444p10le', join(out, `${name}.mov`)],
    ];
  }
  return [[...input, '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '16', '-preset', 'slow', '-movflags', '+faststart', join(out, `${name}.mp4`)]];
}

export function devtoolsUrlFrom(stderrText) {
  const match = /DevTools listening on (ws:\/\/\S+)/.exec(stderrText);
  return match ? match[1] : null;
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

function usage() {
  return `Usage: node tools/render_clips.mjs [NAME ...] [--fps N] [--poster] [--only-poster] [--browser PATH] [--out DIR] [--list]

Renders scenes from video/clips/<name>.html into frame-exact clips in video/out.
With no NAME, every scene in video/clips is rendered.

Options:
  --fps N         override the fps from the scene meta
  --poster        also write <name>.png (middle frame)
  --only-poster   write only <name>.png, no video
  --browser PATH  browser executable (default: Edge, or env CLIPS_BROWSER)
  --out DIR       output directory (default: video/out)
  --list          print parsed meta for each scene and exit
  --help          show this help`;
}

function takeValue(argv, i, arg, name) {
  if (arg.includes('=')) return { value: arg.slice(arg.indexOf('=') + 1), next: i };
  if (i + 1 >= argv.length) throw new Error(`option ${name} needs a value`);
  return { value: argv[i + 1], next: i + 1 };
}

function parseArgs(argv) {
  const opts = { names: [], fps: undefined, poster: false, onlyPoster: false, browser: undefined, out: undefined, list: false, help: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '-h' || arg === '--help') { opts.help = true; continue; }
    if (arg === '--poster') { opts.poster = true; continue; }
    if (arg === '--only-poster') { opts.onlyPoster = true; continue; }
    if (arg === '--list') { opts.list = true; continue; }
    if (arg === '--fps' || arg.startsWith('--fps=')) {
      const v = takeValue(argv, i, arg, '--fps');
      opts.fps = Number(v.value);
      i = v.next;
      continue;
    }
    if (arg === '--browser' || arg.startsWith('--browser=')) {
      const v = takeValue(argv, i, arg, '--browser');
      opts.browser = v.value;
      i = v.next;
      continue;
    }
    if (arg === '--out' || arg.startsWith('--out=')) {
      const v = takeValue(argv, i, arg, '--out');
      opts.out = v.value;
      i = v.next;
      continue;
    }
    if (arg.startsWith('--')) throw new Error(`unknown option ${arg}`);
    opts.names.push(arg);
  }
  if (opts.fps !== undefined && (!Number.isFinite(opts.fps) || opts.fps < 1 || opts.fps > 120)) {
    throw new Error(`--fps must be a number between 1 and 120 (got ${opts.fps})`);
  }
  return opts;
}

function assertValidName(name) {
  if (name.includes('..') || name.includes('/') || name.includes('\\')) {
    throw new Error(`invalid scene name "${name}"`);
  }
}

async function listScenes(clipsDir) {
  let entries;
  try {
    entries = await readdir(clipsDir, { withFileTypes: true });
  } catch {
    throw new Error(`cannot read clips directory ${clipsDir}`);
  }
  return entries
    // `_smoke` and other `_` files are helpers: rendered only when named.
    .filter((entry) => entry.isFile() && entry.name.endsWith('.html') && !entry.name.startsWith('_'))
    .map((entry) => entry.name.slice(0, -'.html'.length))
    .sort();
}

function formatSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

// ---------------------------------------------------------------------------
// CDP client over Node's built-in WebSocket
// ---------------------------------------------------------------------------

function connectCdp(wsUrl) {
  const ws = new WebSocket(wsUrl);
  let nextId = 0;
  const pending = new Map();
  const listeners = new Map();

  const api = {
    ws,
    ready: null,
    send(method, params = {}, sessionId) {
      return new Promise((resolveSend, rejectSend) => {
        const id = ++nextId;
        pending.set(id, { resolve: resolveSend, reject: rejectSend });
        const timer = setTimeout(() => {
          if (pending.delete(id)) rejectSend(new Error(`CDP timeout after ${CDP_TIMEOUT_MS / 1000}s: ${method}`));
        }, CDP_TIMEOUT_MS);
        timer.unref();
        try {
          ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
        } catch (err) {
          clearTimeout(timer);
          pending.delete(id);
          rejectSend(err);
        }
      });
    },
    waitFor(method, timeoutMs = CDP_TIMEOUT_MS) {
      return new Promise((resolveWait, rejectWait) => {
        const remove = () => {
          const set = listeners.get(method);
          if (set) {
            set.delete(handler);
            if (set.size === 0) listeners.delete(method);
          }
        };
        const handler = (msg) => {
          clearTimeout(timer);
          remove();
          resolveWait(msg);
        };
        const timer = setTimeout(() => {
          remove();
          rejectWait(new Error(`timeout waiting for ${method}`));
        }, timeoutMs);
        timer.unref();
        if (!listeners.has(method)) listeners.set(method, new Set());
        listeners.get(method).add(handler);
      });
    },
  };

  api.ready = new Promise((resolveReady, rejectReady) => {
    ws.addEventListener('open', resolveReady, { once: true });
    ws.addEventListener('error', () => rejectReady(new Error(`cannot connect to DevTools WebSocket ${wsUrl}`)), { once: true });
  });

  ws.addEventListener('message', (event) => {
    const text = typeof event.data === 'string' ? event.data : Buffer.from(event.data).toString('utf8');
    const msg = JSON.parse(text);
    if (msg.id !== undefined && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) reject(new Error(`CDP error ${msg.error.code}: ${msg.error.message}`));
      else resolve(msg.result ?? {});
      return;
    }
    if (msg.method) {
      const set = listeners.get(msg.method);
      if (set) for (const handler of [...set]) handler(msg);
    }
  });

  return api;
}

// ---------------------------------------------------------------------------
// Browser and ffmpeg processes
// ---------------------------------------------------------------------------

async function launchBrowser(browserPath) {
  const profileDir = await mkdtemp(join(tmpdir(), 'metamate-clips-'));
  const args = [
    '--headless=new',
    '--remote-debugging-port=0',
    // Some Edge builds refuse the DevTools WebSocket upgrade without an origin allowlist.
    '--remote-allow-origins=*',
    `--user-data-dir=${profileDir}`,
    '--hide-scrollbars',
    '--force-device-scale-factor=1',
    '--mute-audio',
    '--no-first-run',
    '--no-default-browser-check',
    'about:blank',
  ];
  const proc = spawn(browserPath, args, { stdio: ['ignore', 'ignore', 'pipe'] });
  let stderr = '';
  let onData = () => {};
  try {
    const wsUrl = await new Promise((resolveUrl, rejectUrl) => {
      const timer = setTimeout(() => {
        rejectUrl(new Error(`timed out waiting for the DevTools URL from ${browserPath}`));
      }, LAUNCH_TIMEOUT_MS);
      timer.unref();
      onData = (chunk) => {
        stderr += chunk.toString();
        const url = devtoolsUrlFrom(stderr);
        if (url) {
          clearTimeout(timer);
          resolveUrl(url);
        }
      };
      proc.stderr.on('data', onData);
      proc.on('error', (err) => {
        clearTimeout(timer);
        rejectUrl(new Error(`cannot launch browser "${browserPath}": ${err.message}`));
      });
      proc.on('exit', (code) => {
        clearTimeout(timer);
        rejectUrl(new Error(`browser exited before DevTools was ready (code ${code}): ${stderr.slice(-400)}`));
      });
    });
    proc.stderr.off('data', onData);

    const cdp = connectCdp(wsUrl);
    await cdp.ready;
    return { proc, profileDir, cdp };
  } catch (err) {
    proc.kill();
    await rm(profileDir, { recursive: true, force: true }).catch(() => {});
    throw err;
  }
}

function spawnEncoder(args) {
  const proc = spawn('ffmpeg', args, { stdio: ['pipe', 'ignore', 'pipe'] });
  const encoder = { proc, failed: null, stderrTail: '', exited: null };
  encoder.exited = new Promise((resolveExit) => {
    proc.once('exit', (code) => resolveExit(code));
    proc.once('error', (err) => {
      encoder.failed = err;
      resolveExit(null);
    });
  });
  proc.stdin.on('error', (err) => {
    encoder.failed = encoder.failed ?? err;
  });
  proc.stderr.on('data', (chunk) => {
    encoder.stderrTail = (encoder.stderrTail + chunk).slice(-2000);
  });
  return encoder;
}

async function writeFrame(encoder, png) {
  if (encoder.failed) throw encoder.failed;
  if (encoder.proc.stdin.write(png)) return;
  const code = await Promise.race([
    once(encoder.proc.stdin, 'drain').then(() => undefined),
    encoder.exited,
  ]);
  if (encoder.failed) throw encoder.failed;
  if (code !== undefined) {
    throw new Error(`ffmpeg exited early (code ${code}): ${encoder.stderrTail}`);
  }
}

function killEncoder(encoder) {
  try {
    encoder.proc.kill('SIGKILL');
  } catch {
    // already dead
  }
}

async function finishEncoder(encoder) {
  encoder.proc.stdin.end();
  const code = await encoder.exited;
  if (encoder.failed) throw encoder.failed;
  if (code !== 0) throw new Error(`ffmpeg exited with code ${code}: ${encoder.stderrTail}`);
}

async function cleanupRun(run) {
  await Promise.race([
    run.cdp.send('Browser.close').catch(() => {}),
    sleep(3000),
  ]);
  try {
    run.cdp.ws.close();
  } catch {
    // already closed
  }
  if (!run.proc.killed) run.proc.kill();
  await rm(run.profileDir, { recursive: true, force: true }).catch(() => {});
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function seekExpression(t) {
  // Animation currentTime is in milliseconds and already includes the CSS delay,
  // so t * 1000 alone lands every animation exactly on frame k.
  return `(() => {
    const t = ${JSON.stringify(t)};
    for (const animation of document.getAnimations({ subtree: true })) {
      animation.pause();
      animation.currentTime = t * 1000;
    }
    window.__clipFrame?.(t);
  })()`;
}

async function attachPage(run) {
  const { targetId } = await run.cdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await run.cdp.send('Target.attachToTarget', { targetId, flatten: true });
  const page = {
    send: (method, params = {}) => run.cdp.send(method, params, sessionId),
    waitFor: (method, timeoutMs) => run.cdp.waitFor(method, timeoutMs),
  };
  await page.send('Page.enable');
  return { targetId, page };
}

async function renderScene(run, scene, { fps, outDir, video, poster }) {
  const { meta, name } = scene;
  const startedAt = Date.now();
  const { targetId, page } = await attachPage(run);
  try {
    await page.send('Emulation.setDeviceMetricsOverride', {
      width: meta.width,
      height: meta.height,
      deviceScaleFactor: 1,
      mobile: false,
    });
    if (meta.alpha === 1) {
      await page.send('Emulation.setDefaultBackgroundColorOverride', { color: { r: 0, g: 0, b: 0, a: 0 } });
    }

    const url = `${pathToFileURL(scene.file).href}?render=1`;
    const loaded = page.waitFor('Page.loadEventFired', LAUNCH_TIMEOUT_MS);
    await page.send('Page.navigate', { url });
    await loaded;
    await page.send('Runtime.evaluate', { expression: 'document.fonts.ready.then(() => true)', awaitPromise: true });

    const times = frameTimes(meta.duration, fps);
    const posterIndex = Math.floor((times.length - 1) / 2);
    const posterPath = join(outDir, `${name}.png`);
    const argSets = video ? ffmpegArgs({ name, fps, alpha: meta.alpha, out: outDir }) : [];
    const encoders = argSets.map(spawnEncoder);
    const frameIndexes = video ? times.map((_, k) => k) : [posterIndex];
    try {
      for (const k of frameIndexes) {
        await page.send('Runtime.evaluate', { expression: seekExpression(times[k]) });
        const shot = await page.send('Page.captureScreenshot', { format: 'png', fromSurface: true, captureBeyondViewport: false });
        const png = Buffer.from(shot.data, 'base64');
        if (poster && k === posterIndex) await writeFile(posterPath, png);
        for (const encoder of encoders) await writeFrame(encoder, png);
      }
      for (const encoder of encoders) await finishEncoder(encoder);
    } catch (err) {
      for (const encoder of encoders) killEncoder(encoder);
      throw err;
    }

    const seconds = ((Date.now() - startedAt) / 1000).toFixed(1);
    const outputs = argSets.map((args) => args[args.length - 1]);
    if (poster) outputs.push(posterPath);
    const sized = [];
    for (const output of outputs) {
      sized.push(`${output} (${formatSize((await stat(output)).size)})`);
    }
    const frameWord = frameIndexes.length === 1 ? 'frame' : 'frames';
    console.log(`clip ${name}: ${frameIndexes.length} ${frameWord} in ${seconds}s -> ${sized.join(', ')}`);
  } finally {
    await run.cdp.send('Target.closeTarget', { targetId }).catch(() => {});
  }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

async function main() {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (err) {
    console.error(`render_clips: ${err.message}\n`);
    console.error(usage());
    process.exitCode = 1;
    return;
  }
  if (opts.help) {
    console.log(usage());
    return;
  }

  const names = opts.names.length > 0 ? opts.names : await listScenes(CLIPS_DIR);
  const scenes = [];
  for (const name of names) {
    assertValidName(name);
    const file = join(CLIPS_DIR, `${name}.html`);
    let html;
    try {
      html = await readFile(file, 'utf8');
    } catch {
      throw new Error(`scene file not found: ${file}`);
    }
    scenes.push({ name, file, meta: parseClipMeta(html) });
  }

  if (opts.list) {
    for (const scene of scenes) {
      const m = scene.meta;
      console.log(`${scene.name}: duration=${m.duration}s fps=${m.fps} size=${m.width}x${m.height} alpha=${m.alpha}`);
    }
    return;
  }

  const ffmpegCheck = spawnSync('ffmpeg', ['-version'], { stdio: 'ignore' });
  if (ffmpegCheck.error) {
    throw new Error('ffmpeg not found in PATH. Install it with: winget install Gyan.FFmpeg');
  }

  const outDir = opts.out ? resolve(process.cwd(), opts.out) : DEFAULT_OUT_DIR;
  await mkdir(outDir, { recursive: true });

  const browserPath = opts.browser ?? process.env.CLIPS_BROWSER ?? DEFAULT_BROWSER;
  const run = await launchBrowser(browserPath);
  const onSigint = () => {
    try {
      run.proc.kill();
    } catch {
      // already dead
    }
    try {
      rmSync(run.profileDir, { recursive: true, force: true });
    } catch {
      // best effort on the interrupt path
    }
    process.exit(130);
  };
  process.once('SIGINT', onSigint);
  try {
    for (const scene of scenes) {
      await renderScene(run, scene, {
        fps: opts.fps ?? scene.meta.fps,
        outDir,
        video: !opts.onlyPoster,
        poster: opts.poster || opts.onlyPoster,
      });
    }
  } finally {
    process.off('SIGINT', onSigint);
    await cleanupRun(run);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((err) => {
    console.error(`render_clips: ${err.message}`);
    process.exitCode = 1;
  }).finally(() => {
    // Edge's stderr pipe and the DevTools socket can outlive Browser.close on Windows.
    process.exit(process.exitCode ?? 0);
  });
}
