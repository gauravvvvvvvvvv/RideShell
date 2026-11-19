// RideShell anonymous usage counter: a Cloudflare Worker + D1.
//
//   POST /v1/ping   {"id":"<32 hex>","v":"0.1.0","os":"windows","m":5}
//   GET  /v1/stats  Authorization: Bearer <STATS_TOKEN>   (owner dashboard data)
//   GET  /          owner dashboard (asks for the token, keeps it in the browser)
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

function json(value, status = 200) {
  return new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json', 'cache-control': 'no-store' } });
}

function cors(response) {
  const r = new Response(response.body, response);
  r.headers.set('access-control-allow-origin', '*');
  r.headers.set('access-control-allow-methods', 'POST, OPTIONS');
  r.headers.set('access-control-allow-headers', 'content-type');
  return r;
}
