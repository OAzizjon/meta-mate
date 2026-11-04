// Runs only for /media/*. Workers static assets answer every request with the whole file,
// and Safari refuses to play a video from a server that does not honour byte ranges.

export function parseRange(header, size) {
  const match = /^bytes=(\d*)-(\d*)$/.exec((header || '').trim());
  if (!match || (!match[1] && !match[2])) return null;
  let start;
  let end = size - 1;
  if (!match[1]) {
    const suffix = Number(match[2]);
    if (suffix === 0) return 'unsatisfiable';
    start = Math.max(size - suffix, 0);
  } else {
    start = Number(match[1]);
    if (match[2]) end = Math.min(Number(match[2]), size - 1);
  }
  return start > end || start >= size ? 'unsatisfiable' : { start, end };
}

export default {
  async fetch(request, env) {
    const asset = await env.ASSETS.fetch(new Request(request.url, { method: request.method }));
    if (!asset.ok || request.method !== 'GET') return asset;
    const headers = new Headers(asset.headers);
    headers.set('Accept-Ranges', 'bytes');
    const wanted = request.headers.get('Range');
    if (!wanted) return new Response(asset.body, { status: 200, headers });
    // The binding's response carries no length, so a range needs the file in hand.
    const file = await asset.arrayBuffer();
    const range = parseRange(wanted, file.byteLength);
    if (range === null) return new Response(file, { status: 200, headers });
    if (range === 'unsatisfiable') {
      headers.set('Content-Range', `bytes */${file.byteLength}`);
      return new Response(null, { status: 416, headers });
    }
    const length = range.end - range.start + 1;
    headers.set('Content-Range', `bytes ${range.start}-${range.end}/${file.byteLength}`);
    headers.set('Content-Length', String(length));
    return new Response(new Uint8Array(file, range.start, length), { status: 206, headers });
  },
};
