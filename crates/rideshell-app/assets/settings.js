// RideShell App: themes, appearance and the Settings page.
// Colour schemes use the Windows Terminal format, so any WT / PowerShell
// scheme can be pasted in as JSON.
(function () {
  'use strict';
  const $ = (s) => document.querySelector(s);
  const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; };
  const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const I = (n) => window.ICONS[n] || '';

  const scheme = (name, bg, fg, c, b, extra = {}) => ({
    name, background: bg, foreground: fg, cursorColor: extra.cursor || fg, selectionBackground: extra.selection || fg,
    black: c[0], red: c[1], green: c[2], yellow: c[3], blue: c[4], purple: c[5], cyan: c[6], white: c[7],
    brightBlack: b[0], brightRed: b[1], brightGreen: b[2], brightYellow: b[3], brightBlue: b[4], brightPurple: b[5], brightCyan: b[6], brightWhite: b[7],
    builtIn: true, ...extra,
  });

  const SCHEMES = [
    scheme('RideShell Night', '#0d0f18', '#e6e8f0',
      ['#1c1f2b', '#ff6b81', '#4ade80', '#facc15', '#60a5fa', '#c084fc', '#22d3ee', '#cbd5e1'],
      ['#5b6078', '#ff8fa0', '#86efac', '#fde047', '#93c5fd', '#d8b4fe', '#67e8f9', '#f8fafc'], { cursor: '#c4b5fd', selection: '#8b5cf6', accent: ['#a78bfa', '#22d3ee'] }),
    scheme('Campbell', '#0C0C0C', '#CCCCCC',
      ['#0C0C0C', '#C50F1F', '#13A10E', '#C19C00', '#0037DA', '#881798', '#3A96DD', '#CCCCCC'],
      ['#767676', '#E74856', '#16C60C', '#F9F1A5', '#3B78FF', '#B4009E', '#61D6D6', '#F2F2F2'], { cursor: '#FFFFFF', selection: '#FFFFFF', accent: ['#3B78FF', '#61D6D6'] }),
    scheme('Campbell PowerShell', '#012456', '#CCCCCC',
      ['#0C0C0C', '#C50F1F', '#13A10E', '#C19C00', '#0037DA', '#881798', '#3A96DD', '#CCCCCC'],
      ['#767676', '#E74856', '#16C60C', '#F9F1A5', '#3B78FF', '#B4009E', '#61D6D6', '#F2F2F2'], { cursor: '#FFFFFF', selection: '#FFFFFF', accent: ['#3B78FF', '#F9F1A5'] }),
    scheme('One Half Dark', '#282C34', '#DCDFE4',
      ['#282C34', '#E06C75', '#98C379', '#E5C07B', '#61AFEF', '#C678DD', '#56B6C2', '#DCDFE4'],
      ['#5A6374', '#E06C75', '#98C379', '#E5C07B', '#61AFEF', '#C678DD', '#56B6C2', '#DCDFE4'], { accent: ['#61AFEF', '#C678DD'] }),
    scheme('One Half Light', '#FAFAFA', '#383A42',
      ['#383A42', '#E45649', '#50A14F', '#C18301', '#0184BC', '#A626A4', '#0997B3', '#FAFAFA'],
      ['#4F525D', '#DF6C75', '#98C379', '#E4C07A', '#61AFEF', '#C577DD', '#56B5C1', '#FFFFFF'], { cursor: '#4F525D', selection: '#0184BC', accent: ['#0184BC', '#A626A4'] }),
    scheme('Tokyo Night', '#1A1B26', '#C0CAF5',
      ['#15161E', '#F7768E', '#9ECE6A', '#E0AF68', '#7AA2F7', '#BB9AF7', '#7DCFFF', '#A9B1D6'],
      ['#414868', '#F7768E', '#9ECE6A', '#E0AF68', '#7AA2F7', '#BB9AF7', '#7DCFFF', '#C0CAF5'], { accent: ['#7AA2F7', '#BB9AF7'] }),
    scheme('Catppuccin Mocha', '#1E1E2E', '#CDD6F4',
      ['#45475A', '#F38BA8', '#A6E3A1', '#F9E2AF', '#89B4FA', '#F5C2E7', '#94E2D5', '#BAC2DE'],
      ['#585B70', '#F38BA8', '#A6E3A1', '#F9E2AF', '#89B4FA', '#F5C2E7', '#94E2D5', '#A6ADC8'], { cursor: '#F5E0DC', selection: '#585B70', accent: ['#CBA6F7', '#89B4FA'] }),
    scheme('Dracula', '#282A36', '#F8F8F2',
      ['#21222C', '#FF5555', '#50FA7B', '#F1FA8C', '#BD93F9', '#FF79C6', '#8BE9FD', '#F8F8F2'],
      ['#6272A4', '#FF6E6E', '#69FF94', '#FFFFA5', '#D6ACFF', '#FF92DF', '#A4FFFF', '#FFFFFF'], { selection: '#44475A', accent: ['#BD93F9', '#FF79C6'] }),
    scheme('Nord', '#2E3440', '#D8DEE9',
      ['#3B4252', '#BF616A', '#A3BE8C', '#EBCB8B', '#81A1C1', '#B48EAD', '#88C0D0', '#E5E9F0'],
      ['#4C566A', '#BF616A', '#A3BE8C', '#EBCB8B', '#81A1C1', '#B48EAD', '#8FBCBB', '#ECEFF4'], { selection: '#434C5E', accent: ['#88C0D0', '#81A1C1'] }),
    scheme('Solarized Dark', '#002B36', '#839496',
      ['#002B36', '#DC322F', '#859900', '#B58900', '#268BD2', '#D33682', '#2AA198', '#EEE8D5'],
      ['#073642', '#CB4B16', '#586E75', '#657B83', '#839496', '#6C71C4', '#93A1A1', '#FDF6E3'], { selection: '#073642', accent: ['#268BD2', '#2AA198'] }),
    scheme('Gruvbox Dark', '#282828', '#EBDBB2',
      ['#282828', '#CC241D', '#98971A', '#D79921', '#458588', '#B16286', '#689D6A', '#A89984'],
      ['#928374', '#FB4934', '#B8BB26', '#FABD2F', '#83A598', '#D3869B', '#8EC07C', '#EBDBB2'], { selection: '#504945', accent: ['#FABD2F', '#FE8019'] }),
  ];

  const FONTS = [
    ['Geist Mono', 'Geist Mono (built in)'], ['Cascadia Mono', 'Cascadia Mono'], ['Cascadia Code', 'Cascadia Code'],
    ['Consolas', 'Consolas'], ['Lucida Console', 'Lucida Console'], ['JetBrains Mono', 'JetBrains Mono (if installed)'], ['Courier New', 'Courier New'],
  ];

  const DEFAULTS = {
    theme: 'RideShell Night', customSchemes: [], fontFamily: 'Geist Mono', fontSize: 13.5, lineHeight: 1.22,
    cursorStyle: 'bar', cursorBlink: true, copyOnSelect: false, pinned: [], hiddenApps: [], sidebar: true, glass: true,
  };

  const S = (window.RSSettings = {
    values: { ...DEFAULTS },
    schemes() { return [...SCHEMES, ...(this.values.customSchemes || [])]; },
    current() { return this.schemes().find((s) => s.name === this.values.theme) || SCHEMES[0]; },
    load(saved) { this.values = { ...DEFAULTS, ...(saved || {}) }; },
    save() { RS.post({ t: 'saveSettings', settings: this.values }); },
    set(key, value) { this.values[key] = value; this.save(); this.apply(); },
  });

  function hexToRgb(h) { const m = /^#?([0-9a-f]{6})/i.exec(h || ''); if (!m) return [0, 0, 0]; const n = parseInt(m[1], 16); return [n >> 16, (n >> 8) & 255, n & 255]; }
  function luminance(h) { const [r, g, b] = hexToRgb(h); return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255; }
  function alpha(h, a) { const [r, g, b] = hexToRgb(h); return `rgba(${r}, ${g}, ${b}, ${a})`; }
  function mix(h1, h2, t) { const a = hexToRgb(h1), b = hexToRgb(h2); return `rgb(${a.map((v, i) => Math.round(v + (b[i] - v) * t)).join(',')})`; }

  S.xtermTheme = function (sc = S.current()) {
    return {
      background: sc.background, foreground: sc.foreground, cursor: sc.cursorColor || sc.foreground, cursorAccent: sc.background,
      selectionBackground: alpha(sc.selectionBackground || sc.foreground, 0.35), selectionInactiveBackground: alpha(sc.selectionBackground || sc.foreground, 0.18),
      black: sc.black, red: sc.red, green: sc.green, yellow: sc.yellow, blue: sc.blue, magenta: sc.purple, cyan: sc.cyan, white: sc.white,
      brightBlack: sc.brightBlack, brightRed: sc.brightRed, brightGreen: sc.brightGreen, brightYellow: sc.brightYellow,
      brightBlue: sc.brightBlue, brightMagenta: sc.brightPurple, brightCyan: sc.brightCyan, brightWhite: sc.brightWhite,
    };
  };

  S.termOptions = function () {
    const v = S.values;
    // Like Windows Terminal's "adjust indistinguishable colors": keeps text
    // readable when a program prints white on a light scheme.
    const light = luminance(S.current().background) > 0.6;
    return {
      minimumContrastRatio: light ? 4.5 : 1,
      fontFamily: `"${v.fontFamily}", "Geist Mono", "Cascadia Mono", Consolas, monospace`,
      fontSize: v.fontSize, lineHeight: v.lineHeight, cursorStyle: v.cursorStyle, cursorBlink: v.cursorBlink, theme: S.xtermTheme(),
    };
  };

  // Theme the window chrome from the scheme, so the whole app matches.
  S.apply = function () {
    const sc = S.current();
    const light = luminance(sc.background) > 0.6;
    const acc = sc.accent || [sc.blue, sc.purple];
    const r = document.documentElement.style;
    document.body.classList.toggle('light', light);
    r.setProperty('--term-bg', sc.background);
    r.setProperty('--bg', light ? mix(sc.background, '#ffffff', 0.3) : mix(sc.background, '#000000', 0.35));
    r.setProperty('--grad', `linear-gradient(135deg, ${acc[0]} 0%, ${acc[1]} 100%)`);
    r.setProperty('--violet', acc[0]);
    r.setProperty('--violet-2', acc[0]);
    r.setProperty('--cyan', acc[1]);
    r.setProperty('--aurora-1', alpha(acc[0], light ? 0.12 : 0.22));
    r.setProperty('--aurora-2', alpha(acc[1], light ? 0.08 : 0.12));
    const opts = S.termOptions();
    for (const t of RS.tabs.values()) {
      Object.assign(t.term.options, opts);
    }
    requestAnimationFrame(() => RS.fitActive());
    $('#app').classList.toggle('no-sidebar', S.values.sidebar === false);
  };

  // ----- settings page -------------------------------------------------------------

  const SECTIONS = [
    ['appearance', 'Appearance', 'sparkle'],
    ['terminal', 'Terminal', 'terminal'],
    ['sidebar', 'Sidebar', 'sidebar'],
    ['shell', 'Shell & routing', 'terminal'],
    ['accounts', 'Accounts', 'user'],
    ['privacy', 'Privacy & usage', 'shield'],
    ['updates', 'Updates', 'bolt'],
    ['advanced', 'Advanced', 'gear'],
  ];
  let section = 'appearance';

  S.open = function (which) {
    if (which) section = which;
    $('#settings').hidden = false;
    render();
  };
  S.close = function () { $('#settings').hidden = true; RS.focusActive(); };

  function render() {
    const nav = $('#settings-nav');
    nav.innerHTML = '';
    for (const [id, label, icon] of SECTIONS) {
      const b = el('button', 'snav' + (id === section ? ' on' : ''), `${I(icon)}<span>${label}</span>`);
      b.onclick = () => { section = id; render(); };
      nav.appendChild(b);
    }
    const body = $('#settings-body');
    body.innerHTML = '';
    body.scrollTop = 0;
    ({ appearance, terminal, sidebar, shell, accounts, privacy, updates, advanced })[section](body);
  }

  const h = (body, title, sub) => { body.appendChild(el('h3', 'shead', esc(title))); if (sub) body.appendChild(el('p', 'muted ssub', sub)); };
  const card = (body) => { const c = el('div', 'scard'); body.appendChild(c); return c; };
  function row(parent, label, desc, control) {
    const r = el('div', 'srow', `<div class="sl"><div class="st">${esc(label)}</div>${desc ? `<div class="sd">${desc}</div>` : ''}</div>`);
    r.appendChild(control);
    parent.appendChild(r);
    return r;
  }
  function toggle(on, onChange) {
    const t = el('button', 'tog' + (on ? ' on' : ''), '<i></i>');
    t.onclick = () => { on = !on; t.classList.toggle('on', on); onChange(on); };
    return t;
  }
  function select(options, value, onChange) {
    const s = el('select', 'ssel');
    for (const [v, l] of options) { const o = el('option', '', esc(l)); o.value = v; if (String(v) === String(value)) o.selected = true; s.appendChild(o); }
    s.onchange = () => onChange(s.value);
    return s;
  }
  function segmented(options, value, onChange) {
    const g = el('div', 'seg');
    for (const [v, l] of options) {
      const b = el('button', v === value ? 'on' : '', esc(l));
      b.onclick = () => { g.querySelectorAll('button').forEach((x) => x.classList.remove('on')); b.classList.add('on'); onChange(v); };
      g.appendChild(b);
    }
    return g;
  }
  function stepper(value, step, min, max, fmt, onChange) {
    const w = el('div', 'stepper');
    const minus = el('button', '', '−'), plus = el('button', '', '+'), v = el('span', '', fmt(value));
    const setv = (n) => { value = Math.max(min, Math.min(max, Math.round(n * 10) / 10)); v.textContent = fmt(value); onChange(value); };
    minus.onclick = () => setv(value - step); plus.onclick = () => setv(value + step);
    w.append(minus, v, plus);
    return w;
  }

  function preview(sc) {
    const cols = [sc.red, sc.green, sc.yellow, sc.blue, sc.purple, sc.cyan];
    return `<div class="tprev" style="background:${sc.background};color:${sc.foreground}">
      <div class="tline"><span style="color:${sc.blue}">[work]</span> PS C:\\&gt; <span style="color:${sc.yellow}">git</span> status</div>
      <div class="tline"><span style="color:${sc.green}">On branch main</span></div>
      <div class="tline"><span style="color:${sc.red}">modified:</span> app.js</div>
      <div class="tsw">${cols.map((c) => `<i style="background:${c}"></i>`).join('')}</div>
    </div>`;
  }

  function appearance(body) {
    h(body, 'Appearance', 'Colour schemes work like Windows Terminal and PowerShell. Pick one, or import your own.');
    const grid = el('div', 'theme-grid');
    for (const sc of S.schemes()) {
      const c = el('button', 'theme-card' + (sc.name === S.values.theme ? ' on' : ''), `${preview(sc)}<div class="tname2"><span>${esc(sc.name)}</span>${sc.builtIn ? '' : '<em>custom</em>'}</div>`);
      c.onclick = () => { S.set('theme', sc.name); render(); };
      if (!sc.builtIn) {
        c.oncontextmenu = (e) => { e.preventDefault(); S.values.customSchemes = S.values.customSchemes.filter((x) => x.name !== sc.name); if (S.values.theme === sc.name) S.values.theme = DEFAULTS.theme; S.save(); S.apply(); render(); };
        c.title = 'Right-click to delete';
      }
      grid.appendChild(c);
    }
    body.appendChild(grid);

    h(body, 'Import a colour scheme', 'Paste a scheme from Windows Terminal’s settings.json (one scheme, or a list of them), e.g. from windowsterminalthemes.dev.');
    const c = card(body);
    const ta = el('textarea', 'sarea');
    ta.placeholder = '{\n  "name": "My Theme",\n  "background": "#1e1e2e",\n  "foreground": "#cdd6f4",\n  "black": "#45475a", "red": "#f38ba8", ...\n}';
    ta.spellcheck = false;
    const btn = el('button', 'primary', 'Import');
    btn.onclick = () => {
      try {
        let data = JSON.parse(ta.value);
        if (data.schemes) data = data.schemes;
        const list = Array.isArray(data) ? data : [data];
        let n = 0;
        for (const s of list) {
          if (!s.name || !s.background || !s.foreground) throw new Error('each scheme needs name, background and foreground');
          const clean = { ...s, builtIn: false };
          S.values.customSchemes = (S.values.customSchemes || []).filter((x) => x.name !== s.name).concat([clean]);
          n++;
        }
        S.values.theme = list[list.length - 1].name;
        S.save(); S.apply(); render();
        RS.toast(`Imported ${n} scheme${n > 1 ? 's' : ''}`, 'ok');
      } catch (e) { RS.toast('Not a valid scheme: ' + e.message, 'error'); }
    };
    const end = el('div', 'srow-end');
    end.appendChild(btn);
    c.append(ta, end);
  }

  function terminal(body) {
    h(body, 'Terminal', 'How text looks and behaves in every tab.');
    const c = card(body);
    row(c, 'Font', 'Geist Mono is built in. Others must be installed on this PC.', select(FONTS, S.values.fontFamily, (v) => S.set('fontFamily', v)));
    row(c, 'Font size', 'Also Ctrl + = / Ctrl + - in a tab.', stepper(S.values.fontSize, 0.5, 9, 28, (v) => `${v}px`, (v) => S.set('fontSize', v)));
    row(c, 'Line height', '', select([[1, 'Compact (1.0)'], [1.1, '1.1'], [1.22, 'Comfortable (1.22)'], [1.35, '1.35'], [1.5, 'Roomy (1.5)']], S.values.lineHeight, (v) => S.set('lineHeight', Number(v))));
    row(c, 'Cursor', '', segmented([['bar', 'Bar'], ['block', 'Block'], ['underline', 'Underline']], S.values.cursorStyle, (v) => S.set('cursorStyle', v)));
    row(c, 'Blinking cursor', '', toggle(S.values.cursorBlink, (v) => S.set('cursorBlink', v)));
    row(c, 'Copy on select', 'Selected text goes to the clipboard immediately (right-click pastes).', toggle(S.values.copyOnSelect, (v) => S.set('copyOnSelect', v)));
    const k = card(body);
    k.innerHTML = `<div class="keys">
      <div><kbd>Ctrl T</kbd> New tab</div><div><kbd>Ctrl W</kbd> Close tab</div><div><kbd>Ctrl Tab</kbd> Next tab</div>
      <div><kbd>Ctrl K</kbd> Command palette</div><div><kbd>Ctrl B</kbd> Sidebar</div><div><kbd>Ctrl Shift E</kbd> Installed tools</div>
      <div><kbd>Ctrl ,</kbd> Settings</div><div><kbd>Ctrl Shift C</kbd> Copy</div><div><kbd>Ctrl V</kbd> Paste</div></div>`;
  }

  function sidebar(body) {
    h(body, 'Sidebar', 'Choose what the Launch section shows. Pin any installed tool.');
    const c = card(body);
    row(c, 'Show sidebar', 'Ctrl + B', toggle(S.values.sidebar !== false, (v) => S.set('sidebar', v)));
    h(body, 'Built-in launchers');
    const b = card(body);
    for (const a of RS.state?.apps || []) {
      row(b, a.name, a.available ? '' : 'not installed', toggle(!S.values.hiddenApps.includes(a.id), (on) => {
        S.values.hiddenApps = on ? S.values.hiddenApps.filter((x) => x !== a.id) : [...S.values.hiddenApps, a.id];
        S.save(); RS.renderApps();
      }));
    }
    h(body, 'Pinned tools');
    const p = card(body);
    if (!S.values.pinned.length) p.appendChild(el('div', 'sd pad', 'Nothing pinned yet. Open the installed tools list and click ☆ next to a tool.'));
    for (const t of S.values.pinned) {
      const rm = el('button', 'ghost small', 'Remove');
      rm.onclick = () => { S.values.pinned = S.values.pinned.filter((x) => x.name !== t.name); S.save(); RS.renderApps(); render(); };
      row(p, t.name, esc(t.source || ''), rm);
    }
    const add = el('button', 'primary', 'Pin from installed tools…');
    add.onclick = () => { S.close(); RS.openTools(); };
    body.appendChild(el('div', 'srow-end')).appendChild(add);
  }

  function shell(body) {
    const cfg = RS.state?.config;
    h(body, 'Shell & routing', 'How every RideShell tab behaves. Changes apply to new tabs.');
    if (!cfg) { body.appendChild(el('p', 'muted', 'Settings could not be read.')); return; }
    const save = () => RS.post({ t: 'saveConfig', config: cfg });
    const c = card(body);
    row(c, 'PowerShell version', 'Which PowerShell runs inside each tab.', select([['auto', 'Automatic (PowerShell 7 if installed)'], ['pwsh', 'PowerShell 7'], ['powershell', 'Windows PowerShell 5.1']], cfg.session.host, (v) => { cfg.session.host = v; save(); }));
    row(c, 'Run cmd and bash commands automatically', 'dir /s, set X=1 → cmd · ls -la, export X=1 → bash. Prefix @cmd, @bash or @ps to choose yourself.', toggle(cfg.session.auto_switch, (v) => { cfg.session.auto_switch = v; save(); }));
    row(c, 'Show which shell ran a line', 'A dim (cmd) or (bash) marker.', toggle(cfg.session.show_dialect, (v) => { cfg.session.show_dialect = v; save(); }));
    h(body, 'Profiles');
    const d = card(body);
    row(d, 'Show the account in the prompt', 'e.g. [work] PS C:\\>', toggle(cfg.prompt.indicator, (v) => { cfg.prompt.indicator = v; save(); }));
    row(d, 'Switch account by folder', 'Folders bound to an account switch automatically when you cd into them.', toggle(cfg.shell.auto_apply, (v) => { cfg.shell.auto_apply = v; save(); }));
    row(d, 'Announce account switches', 'One short line when the account changes.', toggle(cfg.shell.notify_on_switch, (v) => { cfg.shell.notify_on_switch = v; save(); }));
    row(d, 'Show the account when a routed tool starts', 'e.g. “claude → work” in other terminals.', toggle(cfg.shims.notify, (v) => { cfg.shims.notify = v; save(); }));
    if (cfg.history) row(d, 'Share conversation history with new accounts', 'Off: each new account’s Claude Code / Codex conversations stay its own, so work and personal never mix. On: new accounts also see your existing conversations. Change one account from its ⋯ menu.', toggle(cfg.history.share_new_accounts, (v) => { cfg.history.share_new_accounts = v; save(); }));
  }

  function accounts(body) {
    h(body, 'Accounts', 'Each account keeps its own tool sign-ins, API keys and settings.');
    const c = card(body);
    const profiles = RS.state?.profiles || [];
    if (profiles.length) {
      row(c, 'Default account', 'Used when a folder has no account of its own.', select(profiles.map((p) => [p.name, p.name]), profiles.find((p) => p.isDefault)?.name || '', (v) => RS.post({ t: 'setDefault', name: v })));
    }
    for (const p of profiles) {
      const open = el('button', 'ghost small', 'Open tab');
      open.onclick = () => { S.close(); RS.openTab({ profile: p.name }); };
      row(c, p.name, esc(p.description || '') + (p.tools.length ? ` · ${p.tools.map((t) => t.name).join(', ')}` : ''), open);
    }
    const add = el('button', 'primary', 'New account…');
    add.onclick = () => { S.close(); RS.newProfile(); };
    body.appendChild(el('div', 'srow-end')).appendChild(add);
  }

  const fmtMin = (m) => (m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60 ? `${m % 60} min` : ''}`.trim());

  function updates(body) {
    const U = window.RSUpdate;
    const st = RS.state || {};
    h(body, 'Updates', `You have RideShell ${esc(st.version || '')}. New versions come from the project's GitHub Releases, must carry the project's release signature (Ed25519) and match its signed SHA-256 list before they install, and install without a terminal window.`);
    const c = card(body);
    row(c, 'Check for updates automatically', 'Shortly after start and every 6 hours: one request to GitHub, with no ID or personal data. Nothing installs without your OK.', toggle(S.values.updateCheck !== false, (v) => { S.values.updateCheck = v; S.save(); }));
    const avail = U?.available();
    const check = el('button', avail ? 'primary small' : 'ghost small', avail ? `Install ${esc(avail.version)}…` : 'Check now');
    check.onclick = () => { if (avail) { S.close(); U.offer(); } else { check.textContent = 'Checking…'; U?.check(true); } };
    row(c, avail ? `RideShell ${esc(avail.version)} is available` : 'Check for a new version', avail ? 'Download, verify and install it.' : 'Asks GitHub for the latest release now.', check);
    if (S.values.skipVersion) {
      const un = el('button', 'ghost small', 'Show it again');
      un.onclick = () => { delete S.values.skipVersion; S.save(); render(); };
      row(c, `Skipped version ${esc(S.values.skipVersion)}`, 'You chose not to be reminded about this version.', un);
    }
  }
  window.RSUpdateSettings = () => { if (!$('#settings').hidden && section === 'updates') render(); };

  function privacy(body) {
    const U = window.RSUsage;
    const st = RS.state || {};
    const b = st.build || {};
    h(body, 'Privacy & usage', 'Your usage stays on this PC. RideShell never sends what you run, which folders you use, or anything about your accounts.');

    const u = U ? U.summary() : null;
    if (u) {
      h(body, 'Your usage');
      const tiles = el('div', 'utiles');
      for (const [k, v] of [['Today', fmtMin(u.today)], ['Last 7 days', fmtMin(u.week)], ['Last 30 days', fmtMin(u.month)], ['Sessions', String(u.sessions)]]) {
        tiles.appendChild(el('div', 'utile', `<div class="uk">${k}</div><div class="uv">${esc(v)}</div>`));
      }
      body.appendChild(tiles);
      // One series: minutes per day, last 14 days. Thin bars, rounded tops, hover tooltip, table below.
      const c = card(body);
      const max = Math.max(5, ...u.last14.map((d) => d.minutes));
      const chart = el('div', 'uchart');
      for (const d of u.last14) {
        const col = el('div', 'ucol');
        const pct = (d.minutes / max) * 100;
        col.innerHTML = `<div class="ubar" style="height:${pct}%"></div><div class="ulbl">${d.day.slice(8)}</div>`;
        col.title = `${d.day}: ${fmtMin(d.minutes)}`;
        chart.appendChild(col);
      }
      c.appendChild(el('div', 'ucap', 'Active minutes per day (window in use), last 14 days'));
      c.appendChild(chart);
      const det = el('details', 'utable', '<summary>Show as a table</summary>' + '<table>' + u.last14.slice().reverse().map((d) => `<tr><td>${d.day}</td><td class="n">${fmtMin(d.minutes)}</td></tr>`).join('') + '</table>');
      c.appendChild(det);
      const reset = el('button', 'ghost small', 'Clear');
      reset.onclick = () => { U.reset(); render(); };
      row(card(body), 'Local usage history', `Counted on this PC since ${esc(u.since)}. Only you can see it.`, reset);
    }

    h(body, 'Anonymous usage count');
    const c2 = card(body);
    if (!b.telemetryUrl) {
      row(c2, 'Not part of this build', 'This build contains no usage counter, so nothing is ever sent.', el('span'));
    } else if (b.telemetryBlockedByEnv) {
      row(c2, 'Off (DO_NOT_TRACK or RIDESHELL_NO_TELEMETRY is set)', 'Nothing is sent while that variable is set.', el('span'));
    } else {
      row(c2, 'Help count RideShell users', 'Every 5 minutes while the app is open: a random id that <b>changes every day</b>, the version, the OS family and the active minutes. No commands, folders, files, accounts, tool names or user names. Sent without cookies; the server stores no IP addresses.', toggle(S.values.telemetry !== false, (v) => S.set('telemetry', v)));
      row(c2, 'Server', esc(b.telemetryUrl), el('span'));
    }
    if (b.promoUrl) {
      h(body, 'Messages');
      row(card(body), 'One-line message in the status bar', 'This build shows a short message from the RideShell project. It is fetched without any id, cookie or referrer, so it cannot track you. Builds from source (or forks) can leave it out.', el('span'));
    }
  }

  function advanced(body) {
    const st = RS.state || {};
    h(body, 'Advanced');
    const c = card(body);
    const btn = (label, fn) => { const b = el('button', 'ghost small', label); b.onclick = fn; return b; };
    row(c, 'Version', `RideShell ${esc(st.version)} · ${esc(st.host)}${st.elevated ? ' · Administrator' : ''}`, el('span'));
    row(c, 'Settings file', esc(st.paths?.config || ''), btn('Open', () => RS.post({ t: 'open', what: 'config' })));
    row(c, 'Data folder', esc(st.paths?.data || '') + '<br>Database, account folders, tool routing.', btn('Open', () => RS.post({ t: 'open', what: 'data' })));
    row(c, 'Diagnostics', 'Checks the installation, accounts and shells.', btn('Run', () => { S.close(); RS.openTab({ command: 'rideshell doctor', title: 'Doctor', icon: 'pulse' }); }));
    if (!st.elevated) row(c, 'Administrator', 'Open a second RideShell window with admin rights.', btn('Open', () => { S.close(); RS.openAdmin(); }));
    h(body, 'Explorer');
    const ex = card(body);
    row(ex, 'Open in RideShell', 'Right-click a folder (or the empty space in one) → <b>Show more options</b> → <b>Open in RideShell</b>, at the top. Shift+right-click opens that menu directly. You can also type <code>rideshell</code> in Explorer\'s address bar.', el('span'));
    row(ex, 'Use the classic right-click menu', 'Puts Open in RideShell directly in the first menu (Windows 11 only lists signed, packaged apps there). Affects every app\'s right-click menu; turn off any time. Applies after Explorer restarts.', toggle(!!st.classicMenu, (v) => RS.post({ t: 'classicMenu', on: v })));
    row(ex, 'Restart Explorer', 'Applies the menu setting now. Open Explorer windows close; the taskbar comes back by itself.', btn('Restart', () => { if (confirm('Restart Explorer now? Open Explorer windows will close.')) RS.post({ t: 'restartExplorer' }); }));
    h(body, 'Reset');
    const rc = card(body);
    row(rc, 'Reset appearance', 'Theme, fonts and sidebar back to defaults (accounts are not touched).', btn('Reset', () => { S.values = { ...DEFAULTS }; S.save(); S.apply(); RS.renderApps(); render(); }));
  }

  // Wiring
  $('#settings-close').innerHTML = I('close');
  $('#settings-close').onclick = () => S.close();
  $('#settings').addEventListener('mousedown', (e) => { if (e.target.id === 'settings') S.close(); });
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && !$('#settings').hidden) S.close(); });
})();
