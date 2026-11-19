// RideShell anonymous usage counter: a Cloudflare Worker + D1.
//
//   POST /v1/ping   {"id":"<32 hex>","v":"0.1.0","os":"windows","m":5}
//   GET  /v1/stats  Authorization: Bearer <STATS_TOKEN>   (owner dashboard data)
//   GET  /          owner dashboard (asks for the token, keeps it in the browser)
//   GET  /v1/public, /badge/live.svg, /badge/today.svg   public totals only (README badge)
//
// Privacy: the app sends a random id that changes every UTC day, its version,
// the OS family and active minutes. Nothing else exists to be sent. This
// Worker never stores or logs IP addresses or headers.

import DASHBOARD from './dashboard.html';

const MAX_BODY = 512; // bytes
const OS = new Set(['windows', 'macos', 'linux']);
const LIVE_WINDOW = 10 * 60; // seconds

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    try {
      if (url.pathname === '/v1/ping') {
        if (request.method === 'OPTIONS') return cors(new Response(null, { status: 204 }));
        if (request.method !== 'POST') return json({ error: 'method' }, 405);
        return cors(await ping(request, env));
      }
      if (url.pathname === '/v1/stats' && request.method === 'GET') return await stats(request, env);
      // Public aggregates only (no ids, versions or anything per user): for the README badge.
      if (url.pathname === '/v1/public' && request.method === 'GET') return cors(json(await publicCounts(env), 200, 60));
      if (url.pathname === '/badge/live.svg' && request.method === 'GET') {
        const c = await publicCounts(env);
        return badge('live users', String(c.live), '#16a34a');
      }
      if (url.pathname === '/badge/today.svg' && request.method === 'GET') {
        const c = await publicCounts(env);
        return badge('users today', String(c.today), '#7c4ddb');
      }
      if (url.pathname === '/' && request.method === 'GET') {
        return new Response(DASHBOARD, { headers: { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' } });
      }
      return json({ error: 'not found' }, 404);
    } catch (err) {
      console.error(JSON.stringify({ event: 'error', path: url.pathname, message: String(err && err.message || err) }));
      return json({ error: 'internal' }, 500);
    }
  },
};

async function ping(request, env) {
  const length = Number(request.headers.get('content-length') || '0');
  if (!length || length > MAX_BODY) return json({ error: 'size' }, 413);
  const text = await request.text(); // bounded by MAX_BODY above
  if (text.length > MAX_BODY) return json({ error: 'size' }, 413);
  let body;
  try { body = JSON.parse(text); } catch { return json({ error: 'json' }, 400); }

  const id = typeof body.id === 'string' && /^[0-9a-f]{32}$/.test(body.id) ? body.id : null;
  const version = typeof body.v === 'string' && /^[0-9A-Za-z.+-]{1,24}$/.test(body.v) ? body.v : null;
  const os = OS.has(body.os) ? body.os : null;
  const minutes = Number.isInteger(body.m) && body.m >= 0 && body.m <= 10 ? body.m : null;
  if (!id || !version || !os || minutes === null) return json({ error: 'fields' }, 400);

  const now = Math.floor(Date.now() / 1000);
  const day = new Date(now * 1000).toISOString().slice(0, 10);
  await env.DB.prepare(
    `INSERT INTO pings (day, id, version, os, minutes, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
     ON CONFLICT(day, id) DO UPDATE SET minutes = MIN(1440, minutes + excluded.minutes),
       last_seen = excluded.last_seen, version = excluded.version`,
  ).bind(day, id, version, os, minutes, now).run();
  return new Response(null, { status: 204 });
}

async function publicCounts(env) {
  const now = Math.floor(Date.now() / 1000);
  const today = new Date(now * 1000).toISOString().slice(0, 10);
  const [live, day] = await env.DB.batch([
    env.DB.prepare('SELECT COUNT(*) AS n FROM pings WHERE last_seen > ?1').bind(now - LIVE_WINDOW),
    env.DB.prepare('SELECT COUNT(*) AS n FROM pings WHERE day = ?1').bind(today),
  ]);
  return { live: live.results[0].n, today: day.results[0].n };
}

// A small shields-style SVG badge. Text is generated here, never from input.
function badge(label, value, color) {
  const w = (t) => Math.round(t.length * 6.6 + 12);
  const lw = w(label), vw = w(value);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${lw + vw}" height="20" role="img" aria-label="${label}: ${value}">
<linearGradient id="s" x2="0" y2="100%"><stop offset="0" stop-color="#bbb" stop-opacity=".1"/><stop offset="1" stop-opacity=".1"/></linearGradient>
<clipPath id="r"><rect width="${lw + vw}" height="20" rx="3" fill="#fff"/></clipPath>
<g clip-path="url(#r)"><rect width="${lw}" height="20" fill="#555"/><rect x="${lw}" width="${vw}" height="20" fill="${color}"/><rect width="${lw + vw}" height="20" fill="url(#s)"/></g>
<g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">
<text x="${lw / 2}" y="14">${label}</text><text x="${lw + vw / 2}" y="14">${value}</text></g></svg>`;
  return new Response(svg, { headers: { 'content-type': 'image/svg+xml; charset=utf-8', 'cache-control': 'public, max-age=60' } });
}

async function stats(request, env) {
  if (!env.STATS_TOKEN || !(await tokenMatches(request.headers.get('authorization'), env.STATS_TOKEN))) {
    return json({ error: 'unauthorized' }, 401);
  }
  const now = Math.floor(Date.now() / 1000);
  const today = new Date(now * 1000).toISOString().slice(0, 10);
  const since = new Date((now - 29 * 86400) * 1000).toISOString().slice(0, 10);
  const [live, days, versions, os] = await env.DB.batch([
    env.DB.prepare('SELECT COUNT(*) AS n FROM pings WHERE last_seen > ?1').bind(now - LIVE_WINDOW),
    env.DB.prepare('SELECT day, COUNT(*) AS users, SUM(minutes) AS minutes FROM pings WHERE day >= ?1 GROUP BY day ORDER BY day').bind(since),
    env.DB.prepare('SELECT version, COUNT(*) AS users FROM pings WHERE day = ?1 GROUP BY version ORDER BY users DESC').bind(today),
    env.DB.prepare('SELECT os, COUNT(*) AS users FROM pings WHERE day = ?1 GROUP BY os ORDER BY users DESC').bind(today),
  ]);
  const t = days.results.find((d) => d.day === today) || { users: 0, minutes: 0 };
  return json({
    live: live.results[0].n,
    today: { users: t.users, avgMinutes: t.users ? Math.round(t.minutes / t.users) : 0 },
    days: days.results,
    versions: versions.results,
    os: os.results,
  });
}

// Constant-time comparison of the bearer token (both sides hashed to equal length).
async function tokenMatches(header, secret) {
  const given = (header || '').replace(/^Bearer\s+/i, '');
  const enc = new TextEncoder();
  const [a, b] = await Promise.all([
    crypto.subtle.digest('SHA-256', enc.encode(given)),
    crypto.subtle.digest('SHA-256', enc.encode(secret)),
  ]);
  return crypto.subtle.timingSafeEqual(a, b) && given.length > 0;
}

function json(value, status = 200, maxAge = 0) {
  const cache = maxAge ? `public, max-age=${maxAge}` : 'no-store';
  return new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json', 'cache-control': cache } });
}

function cors(response) {
  const r = new Response(response.body, response);
  r.headers.set('access-control-allow-origin', '*');
  r.headers.set('access-control-allow-methods', 'GET, POST, OPTIONS');
  r.headers.set('access-control-allow-headers', 'content-type');
  return r;
}
