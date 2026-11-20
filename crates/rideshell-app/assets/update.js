// RideShell App: update check, the update wizard and the first-install wizard.
//
// * About 15 s after start and every 6 hours (Settings → Updates can turn it
//   off), the app asks GitHub for the latest release. Newer → a popup.
// * Installing shows a wizard: download → verify (SHA-256) → install (no
//   terminal window) → restart. The work happens in Rust, off the UI thread.
// * `rideshell-app --setup <rideshell.exe>` (what a double-click on the
//   downloaded rideshell.exe opens) shows the same wizard as the installer.
(function () {
  'use strict';
  const $ = (s) => document.querySelector(s);
  const SET = () => window.RSSettings;
  const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const I = (n) => (window.ICONS && window.ICONS[n]) || '';
  const post = (m) => window.RS.post(m);

  let available = null; // latest release info from Rust, when newer
  let mode = null; // 'offer' | 'update' | 'setup'
  let stepNow = null;

  const STEPS = {
    update: [['download', 'Download'], ['verify', 'Verify (SHA-256)'], ['install', 'Install'], ['done', 'Ready']],
    setup: [['install', 'Install'], ['done', 'Ready']],
  };

  // ----- checking -------------------------------------------------------------

  const enabled = () => SET()?.values.updateCheck !== false;
  function check(manual = false) { post({ t: 'updateCheck', manual }); }

  function onUpdate(m) {
    if (m.info) {
      available = m.info;
      if (window.RSUpdateSettings) window.RSUpdateSettings();
      const skipped = SET()?.values.skipVersion === m.info.version;
      if ((m.manual || !skipped) && $('#wizard').hidden) offer();
      return;
    }
    if (m.manual) window.RS.toast(m.error ? `Could not check for updates: ${m.error}` : `RideShell ${window.RS.state?.version || ''} is the latest version`, m.error ? 'error' : 'ok');
  }

  // ----- wizard rendering --------------------------------------------------------------

  function open(title, sub) {
    $('#wz-title').textContent = title;
    $('#wz-sub').innerHTML = sub || '';
    $('#wizard').hidden = false;
  }
  function close() { if (mode === 'setup') return; $('#wizard').hidden = true; mode = null; window.RS.focusActive?.(); }
  function actions(list) {
    const box = $('#wz-actions');
    box.innerHTML = '';
    for (const [label, cls, fn] of list) {
      const b = document.createElement('button');
      b.type = 'button'; b.className = cls; b.textContent = label; b.onclick = fn;
      box.appendChild(b);
    }
    box.lastChild?.focus();
  }
  function renderSteps(kind, current, failed) {
    const ol = $('#wz-steps');
    ol.hidden = false;
    const ids = STEPS[kind].map((s) => s[0]);
    const at = ids.indexOf(current);
    ol.innerHTML = STEPS[kind].map(([id, label], i) => {
      const state = failed && i === at ? 'fail' : i < at || current === 'done' ? 'ok' : i === at ? 'now' : '';
      const mark = state === 'ok' ? '✓' : state === 'fail' ? '!' : String(i + 1);
      return `<li class="${state}"><span class="wz-dot">${mark}</span>${esc(label)}</li>`;
    }).join('');
  }
  function bar(pct) {
    const b = $('#wz-bar');
    b.hidden = pct == null;
    b.classList.toggle('busy', pct === 'busy');
    if (typeof pct === 'number') b.firstElementChild.style.width = `${Math.max(2, pct)}%`;
  }

  // Release notes: plain text with simple bullets, safely escaped.
  function notes(md) {
    // What's new only: stop where the install / verify instructions start.
    const all = String(md || '').split(/\r?\n/);
    const end = all.findIndex((l) => /^\s*(\*\*(Download|Install|Verify)|#+\s*(Download|Install|Verify))/i.test(l));
    const lines = (end >= 0 ? all.slice(0, end) : all).filter((l) => l.trim() && !/^\s*(#|```)/.test(l)).slice(0, 14);
    if (!lines.length) return '';
    const inline = (s) => esc(s).replace(/`([^`]+)`/g, '<code>$1</code>').replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>');
    return '<ul class="wz-notes">' + lines.map((l) => `<li>${inline(l.replace(/^\s*[*-]\s*/, ''))}</li>`).join('') + '</ul>';
  }

  // ----- update -------------------------------------------------------------

  function offer() {
    if (!available) return;
    mode = 'offer';
    open(`RideShell ${available.version} is available`, `You have ${esc(available.current)}. What's new:`);
    $('#wz-body').innerHTML = notes(available.notes) + `<a class="wz-link" href="#" id="wz-release">Full release notes</a>`;
    $('#wz-release').onclick = (e) => { e.preventDefault(); post({ t: 'open', what: available.url }); };
    $('#wz-steps').hidden = true;
    bar(null);
    actions([
      ['Skip this version', 'ghost', () => { SET().values.skipVersion = available.version; SET().save(); close(); }],
      ['Later', 'ghost', close],
      ['Install update', 'primary', startUpdate],
    ]);
  }

  // The update wizard's progress view (also when progress arrives while it is not shown).
  function progressView() {
    mode = 'update';
    open(`Updating to RideShell ${available?.version || ''}`.trim(), 'This takes a moment. You can keep working; nothing restarts until you say so.');
    $('#wz-body').innerHTML = '';
    renderSteps('update', 'download');
    bar(0);
    actions([['Hide', 'ghost', () => { $('#wizard').hidden = true; }]]);
  }
  function startUpdate() {
    progressView();
    post({ t: 'updateInstall' });
  }

  // ----- first install -------------------------------------------------------------

  function showSetup() {
    mode = 'setup';
    document.body.classList.add('setup-mode');
    const v = window.RS.state?.version || '';
    open(`Install RideShell ${v}`, 'One terminal for every shell and every account. Installs for your user only: no admin rights needed.');
    $('#wz-body').innerHTML = `<ul class="wz-notes">
      <li>RideShell App and the <code>rideshell</code> command</li>
      <li>Start menu entries, a Windows Terminal profile and <b>Open in RideShell</b> for folders</li>
      <li>Your tools (<code>claude</code>, <code>codex</code>, <code>gh</code>…) use the right account inside RideShell; other terminals are not changed</li>
      <li>Uninstall any time from <b>Settings → Apps</b></li></ul>`;
    $('#wz-steps').hidden = true;
    bar(null);
    actions([['Cancel', 'ghost', () => post({ t: 'win', op: 'close' })], ['Install', 'primary', () => {
      open(`Installing RideShell ${v}`, 'Setting things up for your user…');
      $('#wz-body').innerHTML = '';
      renderSteps('setup', 'install');
      bar('busy');
      actions([]);
      post({ t: 'setupInstall' });
    }]]);
  }

  // ----- progress from Rust -------------------------------------------------------------

  function onWizard(m) {
    const kind = mode === 'setup' ? 'setup' : 'update';
    if (kind === 'update' && mode !== 'update') progressView();
    if (m.step === 'error') {
      renderSteps(kind, stepNow || STEPS[kind][0][0], true);
      bar(null);
      $('#wz-title').textContent = kind === 'setup' ? 'RideShell could not be installed' : 'The update could not be installed';
      $('#wz-sub').textContent = 'Your current RideShell keeps working as it is.';
      $('#wz-body').innerHTML = `<p class="wz-error">${esc(m.error)}</p>`;
      $('#wizard').hidden = false;
      actions(kind === 'setup'
        ? [['Close', 'ghost', () => post({ t: 'win', op: 'close' })], ['Try again', 'primary', showSetup]]
        : [['Close', 'ghost', close], ['Try again', 'primary', startUpdate]]);
      return;
    }
    stepNow = m.step;
    renderSteps(kind, m.step);
    if (m.step === 'download') bar(m.pct ?? 0);
    else if (m.step === 'done') bar(null);
    else bar('busy');
    if (m.step === 'done') {
      $('#wizard').hidden = false;
      if (kind === 'setup') {
        open('RideShell is installed', 'Find it in the Start menu, or right-click a folder → <b>Open in RideShell</b>.');
        actions([['Close', 'ghost', () => post({ t: 'win', op: 'close' })], ['Open RideShell', 'primary', () => post({ t: 'relaunch' })]]);
      } else {
        open(`RideShell ${available?.version || ''} is installed`, 'Restart to use it. Open tabs close, so finish what is running first.');
        actions([['Later', 'ghost', close], ['Restart now', 'primary', () => post({ t: 'relaunch' })]]);
      }
    }
  }

  // ----- wiring -------------------------------------------------------------

  $('#wz-logo').innerHTML = I('logo');
  $('#wizard').addEventListener('mousedown', (e) => { if (e.target.id === 'wizard' && mode === 'offer') close(); });

  let started = false;
  function start(state) {
    if (started) return;
    started = true;
    if (state?.startup?.setup) { showSetup(); return; }
    setTimeout(() => { if (enabled()) check(); }, 15000);
    setInterval(() => { if (enabled()) check(); }, 6 * 3600000);
  }

  window.RSUpdate = { start, check, onUpdate, onWizard, offer, available: () => available };
})();
