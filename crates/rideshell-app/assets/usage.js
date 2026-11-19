// RideShell App: local usage stats, the optional anonymous counter, and the
// optional one-line message slot in the status bar.
//
// * Local stats (minutes per day, sessions) stay in app.json on this PC.
// * The counter exists only in builds made with RIDESHELL_TELEMETRY_URL. It
//   sends a random id that changes every UTC day, the version, the OS family
//   and active minutes. Users can turn it off; DO_NOT_TRACK is respected.
// * The message slot exists only in builds made with RIDESHELL_PROMO_URL. It
//   fetches one short text and a link, with no id, cookies or referrer.
(function () {
  'use strict';
  const $ = (s) => document.querySelector(s);
  const SET = () => window.RSSettings;
  const today = () => new Date().toISOString().slice(0, 10); // UTC, matches the server

  // "Active" = the window has focus, or there was input in the last 5 minutes.
  let lastInput = Date.now();
  for (const ev of ['keydown', 'mousedown', 'wheel']) window.addEventListener(ev, () => { lastInput = Date.now(); }, { capture: true, passive: true });
  const active = () => document.hasFocus() || Date.now() - lastInput < 5 * 60000;

  let started = false;
  let pendingMinutes = 0; // active minutes not yet reported to the counter
  let dirty = false;

  function usage() {
    const v = SET().values;
    v.usage = v.usage || { days: {}, sessions: 0, since: today() };
    return v.usage;
  }

  function start() {
    if (started || !SET() || !window.RS?.state) return;
    started = true;
    const u = usage();
    u.sessions += 1;
    dirty = true;
    maybeNotice();
    setInterval(tick, 60000);
    setInterval(() => { if (dirty) { SET().save(); dirty = false; } }, 5 * 60000);
    setTimeout(ping, 15000);
    setInterval(ping, 5 * 60000);
    loadPromo();
    setInterval(loadPromo, 6 * 3600000);
    window.addEventListener('beforeunload', () => { if (dirty) SET().save(); });
  }

  function tick() {
    if (!active()) return;
    const u = usage();
    const d = today();
    u.days[d] = (u.days[d] || 0) + 1;
    // Keep 180 days.
    const keys = Object.keys(u.days).sort();
    while (keys.length > 180) delete u.days[keys.shift()];
    pendingMinutes += 1;
    dirty = true;
  }

  // ----- anonymous counter -----------------------------------------------------------

  function counterEnabled() {
    const b = window.RS.state?.build || {};
    return !!b.telemetryUrl && !b.telemetryBlockedByEnv && SET().values.telemetry !== false;
  }

  function dailyId() {
    const v = SET().values;
    const d = today();
    if (!v.telemetryDay || v.telemetryDay.date !== d) {
      const bytes = crypto.getRandomValues(new Uint8Array(16));
      v.telemetryDay = { date: d, id: [...bytes].map((b) => b.toString(16).padStart(2, '0')).join('') };
      dirty = true;
    }
    return v.telemetryDay.id;
  }

  async function ping() {
    if (!counterEnabled()) { pendingMinutes = 0; return; }
    const b = window.RS.state.build;
    const m = Math.min(10, pendingMinutes);
    pendingMinutes -= m;
    try {
      await fetch(b.telemetryUrl.replace(/\/$/, '') + '/v1/ping', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ id: dailyId(), v: window.RS.state.version, os: b.os, m }),
        credentials: 'omit',
        referrerPolicy: 'no-referrer',
        cache: 'no-store',
        keepalive: true,
      });
    } catch { pendingMinutes += m; /* offline: report later */ }
  }

  function maybeNotice() {
    const b = window.RS.state?.build || {};
    const v = SET().values;
    if (!b.telemetryUrl || b.telemetryBlockedByEnv || v.telemetryNoticeShown) return;
    const seen = () => { SET().values.telemetryNoticeShown = true; SET().save(); };
    const t = document.createElement('div');
    t.className = 'toast notice';
    t.innerHTML = '<div><b>Anonymous usage count.</b> RideShell counts how many people use it and for how long, with a random id that changes every day. Nothing about what you do is sent.</div>';
    const row = document.createElement('div');
    row.className = 'notice-actions';
    const off = document.createElement('button'); off.className = 'ghost small'; off.textContent = 'Turn off';
    off.onclick = () => { seen(); SET().set('telemetry', false); t.remove(); };
    const more = document.createElement('button'); more.className = 'ghost small'; more.textContent = 'Details';
    more.onclick = () => { seen(); t.remove(); SET().open('privacy'); };
    const ok = document.createElement('button'); ok.className = 'primary small'; ok.textContent = 'OK';
    ok.onclick = () => { seen(); t.remove(); };
    row.append(off, more, ok);
    t.appendChild(row);
    $('#toasts').appendChild(t);
  }

  // ----- message slot -----------------------------------------------------------

  async function loadPromo() {
    const url = window.RS.state?.build?.promoUrl;
    const el = $('#sb-promo');
    if (!url) { el.hidden = true; return; }
    try {
      const res = await fetch(url, { credentials: 'omit', referrerPolicy: 'no-referrer', cache: 'no-store' });
      if (!res.ok) return;
      const p = await res.json(); // { "text": "...", "url": "https://...", "until": "YYYY-MM-DD" }
      if (!p || typeof p.text !== 'string' || !p.text.trim() || (p.until && p.until < today())) { el.hidden = true; return; }
      el.textContent = p.text.trim().slice(0, 120);
      const link = typeof p.url === 'string' && p.url.startsWith('https://') ? p.url : null;
      el.title = link ? link : '';
      el.classList.toggle('link', !!link);
      el.onclick = (e) => { e.preventDefault(); if (link) window.RS.post({ t: 'open', what: link }); };
      el.hidden = false;
    } catch { /* offline: keep whatever is shown */ }
  }

  // ----- local stats for the Settings page -----------------------------------------------------------

  window.RSUsage = {
    summary() {
      const u = usage();
      const days = u.days;
      const sumSince = (n) => {
        let total = 0;
        for (let i = 0; i < n; i++) total += days[new Date(Date.now() - i * 86400000).toISOString().slice(0, 10)] || 0;
        return total;
      };
      const all = Object.values(days).reduce((a, b) => a + b, 0);
      const last14 = [];
      for (let i = 13; i >= 0; i--) {
        const d = new Date(Date.now() - i * 86400000).toISOString().slice(0, 10);
        last14.push({ day: d, minutes: days[d] || 0 });
      }
      return { today: days[today()] || 0, week: sumSince(7), month: sumSince(30), all, sessions: u.sessions, since: u.since, last14 };
    },
    reset() { const v = SET().values; v.usage = { days: {}, sessions: 1, since: today() }; SET().save(); },
    counterEnabled,
    flushIfDirty() { if (dirty) { SET().save(); dirty = false; } },
  };

  // Start once the first state (and settings) arrived.
  const wait = setInterval(() => { if (window.RS?.state && SET()) { clearInterval(wait); start(); } }, 500);
})();
