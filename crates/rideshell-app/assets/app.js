// RideShell App UI. Talks to the Rust host through window.ipc.postMessage
// (JSON) and receives messages through RS.recv / RS.data.
(function () {
  'use strict';

  const $ = (sel) => document.querySelector(sel);
  const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; };
  const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const post = (msg) => window.ipc.postMessage(JSON.stringify(msg));
  const I = (name) => (window.ICONS[name] || '');
  const store = {
    get(k, d) { try { const v = localStorage.getItem('rs.' + k); return v == null ? d : JSON.parse(v); } catch { return d; } },
    set(k, v) { try { localStorage.setItem('rs.' + k, JSON.stringify(v)); } catch { /* ignore */ } },
  };

  const PALETTE = [
    ['#a78bfa', '#7c3aed'], ['#22d3ee', '#0891b2'], ['#f472b6', '#db2777'], ['#34d399', '#059669'],
    ['#fbbf24', '#d97706'], ['#60a5fa', '#2563eb'], ['#fb7185', '#e11d48'], ['#a3e635', '#65a30d'],
  ];
  function hash(s) { let h = 2166136261; for (const c of s) { h ^= c.charCodeAt(0); h = Math.imul(h, 16777619); } return h >>> 0; }
  function colorOf(name) { const [a, b] = PALETTE[hash(name || '') % PALETTE.length]; return { solid: a, grad: `linear-gradient(135deg, ${a}, ${b})` }; }

  // Programs show their own icon (read from the .exe by the host). Programs
  // without one get the same neutral placeholder, tinted per program.
  const ACCENTS = ['#818cf8', '#2dd4bf', '#f472b6', '#fbbf24', '#60a5fa', '#fb7185', '#a3e635', '#c084fc', '#38bdf8', '#fb923c'];
  function accentOf(key) { return ACCENTS[hash(String(key || '').toLowerCase()) % ACCENTS.length]; }
  function exeIcon(path, key, cls = '') {
    const img = path ? `<img src="/exe-icon?p=${encodeURIComponent(path)}" alt="" draggable="false" loading="lazy" onload="this.parentNode.classList.add('has-img')" onerror="this.remove()">` : '';
    return `<span class="aicon ${cls}" style="--ac:${accentOf(key)}">${I('app')}${img}</span>`;
  }
  // `t` is a connected tool ({id, tool, ...}) or a provider id.
  function toolIcon(t, cls) {
    const id = typeof t === 'string' ? t : t.id;
    const cmd = typeof t === 'string' ? t : t.tool;
    const prov = (RS.state?.providers || []).find((p) => p.id === id);
    const fromTools = tools.list.find((x) => x.name.toLowerCase() === String(cmd).toLowerCase());
    return exeIcon(RS.state?.toolIcons?.[id] || prov?.icon || fromTools?.path, cmd || id, cls);
  }
  const toolName = (id) => (RS.state?.providers || []).find((p) => p.id === id)?.name || id;


  const RS = window.RS = {
    state: null,
    tabs: new Map(),
    order: [],
    active: null,
    nextId: 1,
  };
  const SET = () => window.RSSettings;

  // ----- messages from Rust -----------------------------------------------------------

  const b64ToBytes = (b64) => { const bin = atob(b64); const out = new Uint8Array(bin.length); for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i); return out; };

  RS.data = (id, b64) => { const t = RS.tabs.get(id); if (t) t.term.write(b64ToBytes(b64)); };

  RS.recv = (m) => {
    switch (m.t) {
      case 'state': onState(m.state); break;
      case 'stateLite': RS.state = { ...RS.state, classicMenu: m.state.classicMenu }; break;
      case 'exit': onExit(m.id, m.code); break;
      case 'spawnError': { const t = RS.tabs.get(m.id); if (t) { t.ended = true; t.term.write(`\r\n\x1b[38;5;210m${m.text}\x1b[0m\r\n`); renderTabs(); } break; }
      case 'toast': toast(m.text, m.kind); break;
      case 'created': toast(`Account “${m.name}” created`, 'ok'); openTab({ profile: m.name }); break;
      case 'tools': onTools(m); break;
      case 'picked': openTab({ cwd: m.path }); break;
      case 'dropped': onDropped(m.items); break;
      case 'dir': onDir(m); break;
      case 'worked': RS.worked = m.folders || []; if (!$('#pane-files').hidden) renderFolderLists(); break;
      case 'paste': { const t = RS.tabs.get(m.id); if (t && !t.ended) t.term.paste(m.text); break; }
    }
  };

  let firstState = true;
  function onState(s) {
    RS.state = s;
    if (firstState) { SET().load(s.settings); SET().apply(); applyCollapsed(); }
    $('#admin-badge').hidden = !s.elevated;
    $('#admin-btn').hidden = s.elevated;
    $('#sb-version').textContent = `RideShell ${s.version}`;
    $('#sb-shells').innerHTML = `${I('terminal')}${esc(s.host)} · cmd${s.bash ? ' · bash' : ''}`;
    if (s.error) toast(s.error, 'error');
    renderProfiles();
    renderApps();
    renderStatus();
    renderFiles();
    renderFavorites();
    if (firstState) {
      firstState = false;
      const st = s.startup || {};
      openTab({ profile: st.profile || null, cwd: st.dir || null });
    }
  }

  function defaultProfile() { return (RS.state?.profiles || []).find((p) => p.isDefault)?.name || null; }
  function activeTab() { return RS.tabs.get(RS.active) || null; }
  function activeProfile() { const t = activeTab(); return (t && (t.live.profile || t.profile)) || defaultProfile(); }

  // ----- tabs & terminals -------------------------------------------------------------

  function openTab({ profile = null, cwd = null, command = null, title = null, icon = null } = {}) {
    const id = RS.nextId++;
    const host = el('div', 'term');
    $('#terminals').appendChild(host);
    const term = new Terminal({
      fontWeight: 400, fontWeightBold: 640, cursorWidth: 2, scrollback: 10000,
      allowProposedApi: true, drawBoldTextInBrightColors: false,
      windowsPty: { backend: 'conpty', buildNumber: 22621 },
      ...SET().termOptions(),
    });
    const fit = new FitAddon.FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon.WebLinksAddon((_e, uri) => post({ t: 'open', what: uri })));
    term.open(host);
    try { const gl = new WebglAddon.WebglAddon(); gl.onContextLoss(() => gl.dispose()); term.loadAddon(gl); } catch { /* DOM renderer fallback */ }

    const tab = { id, term, fit, host, profile, cwd, command, title: title || 'PowerShell', icon: icon || 'ps', ended: false, atPrompt: false, live: { profile: null, cwd: null, admin: false } };
    RS.tabs.set(id, tab);
    RS.order.push(id);

    term.parser.registerOscHandler(7777, (data) => {
      const pi = data.indexOf('profile='), ai = data.indexOf(';admin='), ci = data.indexOf(';cwd=');
      if (pi === 0 && ai > 0 && ci > ai) {
        tab.live.profile = data.slice(8, ai) || null;
        tab.live.admin = data.slice(ai + 7, ci) === '1';
        const prev = tab.live.cwd;
        tab.live.cwd = data.slice(ci + 5);
        tab.atPrompt = true;
        if (tab.live.cwd !== prev) rememberFolder(tab.live.cwd);
        if (tab.id === RS.active) { renderStatus(); renderProfiles(); if (tab.live.cwd !== prev) followTab(); }
        renderTabs();
      }
      return true;
    });
    term.onData((d) => {
      if (tab.ended) { if (d === '\r') restartTab(tab); return; }
      if (d.includes('\r')) tab.atPrompt = false;
      post({ t: 'input', id, data: d });
    });
    term.onResize(({ cols, rows }) => { if (!tab.ended) post({ t: 'resize', id, cols, rows }); });
    term.attachCustomKeyEventHandler((e) => keyHandler(e, tab));
    term.onSelectionChange(() => { if (SET().values.copyOnSelect && term.hasSelection()) navigator.clipboard.writeText(term.getSelection()).catch(() => {}); });
    host.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      if (term.hasSelection()) { navigator.clipboard.writeText(term.getSelection()).catch(() => {}); term.clearSelection(); }
      else post({ t: 'paste', id });
    });

    activate(id);
    requestAnimationFrame(() => {
      fit.fit();
      post({ t: 'spawn', id, profile, cwd, command, cols: term.cols, rows: term.rows });
    });
    return tab;
  }

  function restartTab(tab) {
    tab.ended = false;
    tab.term.reset();
    post({ t: 'spawn', id: tab.id, profile: tab.live.profile || tab.profile, cwd: tab.live.cwd || tab.cwd, command: null, cols: tab.term.cols, rows: tab.term.rows });
    renderTabs();
  }

  function onExit(id, code) {
    const tab = RS.tabs.get(id);
    if (!tab) return;
    if (code === 0 && !tab.command) { closeTab(id); return; }
    tab.ended = true;
    tab.term.write(`\r\n\x1b[38;5;245m── session ended${code != null ? ` (exit code ${code})` : ''} · Enter to restart · Ctrl+W to close ──\x1b[0m\r\n`);
    renderTabs();
  }

  function activate(id) {
    RS.active = id;
    for (const t of RS.tabs.values()) t.host.classList.toggle('active', t.id === id);
    const t = RS.tabs.get(id);
    $('#empty').hidden = RS.tabs.size > 0;
    renderTabs(); renderStatus(); renderProfiles(); followTab();
    if (t) requestAnimationFrame(() => { t.fit.fit(); t.term.focus(); });
  }

  function closeTab(id) {
    const t = RS.tabs.get(id);
    if (!t) return;
    if (!t.ended) post({ t: 'kill', id });
    t.term.dispose(); t.host.remove();
    RS.tabs.delete(id);
    const i = RS.order.indexOf(id);
    RS.order.splice(i, 1);
    if (RS.active === id) {
      const next = RS.order[Math.min(i, RS.order.length - 1)];
      if (next != null) activate(next); else { RS.active = null; $('#empty').hidden = false; renderTabs(); renderStatus(); renderProfiles(); }
    } else renderTabs();
  }

  function cycle(step) {
    if (!RS.order.length) return;
    const i = RS.order.indexOf(RS.active);
    activate(RS.order[(i + step + RS.order.length) % RS.order.length]);
  }

  function renderTabs() {
    const nav = $('#tabs');
    nav.innerHTML = '';
    for (const id of RS.order) {
      const t = RS.tabs.get(id);
      const prof = t.live.profile || t.profile || defaultProfile();
      const b = el('div', 'tab' + (id === RS.active ? ' active' : '') + (t.ended ? ' ended' : ''));
      if (prof) b.style.setProperty('--tab-color', colorOf(prof).solid);
      b.innerHTML = `<span class="dot"></span><span class="label">${esc(t.title)}${prof ? `<small>${esc(prof)}</small>` : ''}</span><button class="x" title="Close (Ctrl+W)">${I('close')}</button>`;
      b.title = (t.live.cwd || '') + (t.live.admin ? '  · Administrator' : '');
      b.addEventListener('mousedown', (e) => { if (e.button === 1) { e.preventDefault(); closeTab(id); } });
      b.addEventListener('click', (e) => { if (e.target.closest('.x')) closeTab(id); else activate(id); });
      nav.appendChild(b);
    }
  }

  // ----- accounts -------------------------------------------------------------

  function switchAccount(name) {
    const t = activeTab();
    if (!t || t.ended) { openTab({ profile: name }); return; }
    // Esc clears whatever is typed (PSReadLine), then the session switches.
    post({ t: 'input', id: t.id, data: '\x1b' + `rideshell profile use ${name}\r` });
    t.live.profile = name;
    renderTabs(); renderProfiles(); renderStatus();
    t.term.focus();
  }

  function runInNewTab(command, opts = {}) { return openTab({ command, ...opts }); }

  function renderProfiles() {
    const box = $('#profiles');
    if (!RS.state) return;
    box.innerHTML = '';
    const current = activeProfile();
    const profiles = RS.state.profiles;
    if (!profiles.length) {
      const e = el('div', 'tool-row', `<span class="t-name">No accounts yet</span>`);
      box.appendChild(e);
    }
    for (const p of profiles) {
      const c = colorOf(p.name);
      const row = el('div', 'profile' + (p.name === current ? ' active' : ''));
      const chips = p.tools.slice(0, 8).map((t) => `<span title="${esc(t.name)}">${toolIcon(t, 'tiny')}</span>`).join('');
      row.innerHTML = `
        <div class="avatar" style="--c:${c.grad}">${esc(p.name.slice(0, 1))}</div>
        <div class="meta">
          <div class="name">${esc(p.name)}${p.isDefault ? '<span class="pill">default</span>' : ''}${p.name === current ? '<span class="pill live">active</span>' : ''}</div>
          ${p.description ? `<div class="desc">${esc(p.description)}</div>` : ''}
          ${p.tools.length ? `<div class="tools">${chips}${p.tools.length > 8 ? `<span class="more">+${p.tools.length - 8}</span>` : ''}</div>` : ''}
        </div>
        <button class="open" title="New tab as ${esc(p.name)}">${I('plus')}</button>`;
      row.addEventListener('click', (e) => {
        if (e.target.closest('.open')) openTab({ profile: p.name });
        else switchAccount(p.name);
      });
      row.addEventListener('contextmenu', (e) => { e.preventDefault(); profileMenu(p, e.clientX, e.clientY); });
      box.appendChild(row);

      if (p.name === current) {
        const d = el('div', 'details');
        for (const t of p.tools) {
          const badge = t.history === 'original' ? '<i class="hb" title="Uses your original folder: its sign-in and all your conversations"></i>'
            : t.history === 'shared' ? '<i class="hb shared" title="Own sign-in, shared conversation history"></i>' : '';
          const r = el('div', 'tool-row', `${toolIcon(t, 'tiny')}<span class="t-name" title="${esc(t.summary)}">${esc(t.name)}</span>${badge}`);
          if (t.login) {
            const login = el('button', '', 'Sign in');
            login.title = `Run ${t.tool}'s own sign-in for ${p.name}`;
            login.onclick = () => signIn(p.name, t.id, t.name);
            r.appendChild(login);
          }
          if (t.keyCmd) {
            const key = el('button', '', 'API key');
            key.title = `Set ${t.keyVars.join(', ')} for ${p.name}`;
            key.onclick = () => changeKey(p, t);
            r.appendChild(key);
          }
          const more = el('button', 'tmore', '⋯');
          more.title = 'More';
          more.onclick = (e) => { const rc = e.currentTarget.getBoundingClientRect(); toolMenu(p, t, rc.left, rc.bottom + 4); };
          r.appendChild(more);
          r.addEventListener('contextmenu', (e) => { e.preventDefault(); toolMenu(p, t, e.clientX, e.clientY); });
          d.appendChild(r);
        }
        const add = el('button', 'add-tool', `${I('plus')}<span>Connect a tool</span>`);
        add.onclick = () => openConnect(p);
        d.appendChild(add);
        box.appendChild(d);
      }
    }
  }

  function changeKey(p, t) {
    if (!t.keyCmd) return;
    runInNewTab(t.keyCmd, { profile: p.name, title: `API key · ${t.name}`, icon: 'key' });
    setTimeout(() => post({ t: 'state' }), 4000);
  }

  // The per-tool menu: everything about one tool account lives here.
  function toolMenu(p, t, x, y) {
    const items = [];
    if (t.login) items.push({ html: toolIcon(t, 'tiny'), label: `Sign in to ${t.name}`, run: () => signIn(p.name, t.id, t.name) });
    if (t.status) items.push({ icon: 'pulse', label: 'Check sign-in status', run: () => runInNewTab(`rideshell auth status ${t.id} --profile ${p.name}`, { profile: p.name, title: `Status · ${t.name}`, icon: 'pulse' }) });
    if (t.keyCmd) items.push({ icon: 'key', label: `API key (${t.keyVars.join(', ')})`, run: () => changeKey(p, t) });
    if (t.history) {
      items.push('-');
      const n = t.conversations ?? 0;
      if (t.history !== 'original') {
        items.push({ icon: 'folder', label: `Use original ${t.name} folder (sign-in + all conversations)`, run: () => historyAction('adopt', p, t) });
      }
      if (t.history === 'own') items.push({ icon: 'branch', label: 'Share conversation history (keep own sign-in)', run: () => historyAction('share', p, t) });
      if (t.history === 'shared') items.push({ icon: 'close', label: 'Stop sharing history', run: () => historyAction('separate', p, t) });
      items.push({ icon: 'doc', label: `${n} conversation${n === 1 ? '' : 's'} here · details`, run: () => runInNewTab('rideshell history status', { title: 'History', icon: 'doc' }) });
    }
    items.push('-');
    items.push({ icon: 'close', label: `Disconnect ${t.name}`, run: () => {
      if (confirm(`Disconnect ${t.name} from ${p.name}?`)) {
        runInNewTab(`rideshell identity unset ${p.name} ${t.id}`, { profile: p.name, title: `Disconnect · ${t.name}`, icon: 'close' });
      }
    } });
    menu(items, x, y);
  }

  function historyAction(action, p, t) {
    const text = {
      adopt: `${p.name} will use your original ${t.name} folder: the sign-in there and all its conversations. Conversations ${p.name} had in its own folder are moved there first. Continue?`,
      share: `${p.name} keeps its own ${t.name} sign-in but will see the same conversations as your original folder. Its own conversations are added to them. Continue?`,
      separate: `${p.name} gets its own ${t.name} history again. The shared conversations stay where they are. Continue?`,
    }[action];
    if (!confirm(text)) return;
    const yes = action === 'adopt' ? ' --yes' : '';
    runInNewTab(`rideshell history ${action} ${t.id} --profile ${p.name}${yes}`, { profile: p.name, title: `History · ${t.name}`, icon: 'doc' });
    setTimeout(() => post({ t: 'state' }), 2500);
  }

  // ----- connect a tool -------------------------------------------------------------

  // Connecting is a plain terminal command: it asks a few questions there.
  function openConnect(p) {
    runInNewTab(`rideshell connect --profile ${p.name}`, { profile: p.name, title: 'Connect a tool', icon: 'plus' });
    setTimeout(() => post({ t: 'state' }), 8000);
  }

  // ----- sign-in guide -------------------------------------------------------------

  let signing = null;
  function signIn(profile, tool, displayName) {
    signing = { profile, tool };
    const name = displayName || toolName(tool);
    $('#si-title').textContent = `Sign ${profile} in to ${name}`;
    $('#si-text').innerHTML = `Runs ${esc(name)}'s own sign-in for <b>${esc(profile)}</b>.`;
    const tips = [
      `If your browser is already signed in to ${name} with another account, use <b>switch account</b> or <b>sign out</b> on the page that opens, then sign in with the account you want.`,
      'The sign-in link is also shown in the terminal: you can paste it into another browser or browser profile.',
    ];
    $('#si-tips').innerHTML = tips.map((t) => `<li>${t}</li>`).join('');
    $('#si-email-row').hidden = tool !== 'claude';
    $('#si-email').value = '';
    document.querySelector('input[name="si-browser"][value="default"]').checked = true;
    $('#signin').hidden = false;
    setTimeout(() => (tool === 'claude' ? $('#si-email') : $('#signin .primary')).focus(), 30);
  }
  $('#signin-form').addEventListener('submit', (e) => {
    e.preventDefault();
    if (!signing) return;
    const { profile, tool } = signing;
    const browser = document.querySelector('input[name="si-browser"]:checked').value;
    const email = $('#si-email').value.trim();
    let cmd = `rideshell auth login ${tool} --profile ${profile}`;
    if (browser === 'private') cmd += ' --browser private';
    if (email && tool === 'claude') cmd += ` --email '${email.replace(/'/g, "''")}'`;
    $('#signin').hidden = true;
    signing = null;
    runInNewTab(cmd, { profile, title: `Sign in · ${toolName(tool)}`, icon: 'login' });
  });
  $('#si-cancel').onclick = () => { $('#signin').hidden = true; signing = null; activeTab()?.term.focus(); };
  $('#signin').addEventListener('mousedown', (e) => { if (e.target.id === 'signin') { $('#signin').hidden = true; signing = null; } });

  function profileMenu(p, x, y) {
    const items = [
      { icon: 'check', label: 'Use in this tab', run: () => switchAccount(p.name) },
      { icon: 'plus', label: 'Open a new tab', run: () => openTab({ profile: p.name }) },
    ];
    if (!p.isDefault) items.push({ icon: 'star', label: 'Make default', run: () => post({ t: 'setDefault', name: p.name }) });
    items.push('-');
    items.push({ icon: 'user', label: 'Rename…', run: () => {
      const n = prompt(`New name for ${p.name}:`, p.name);
      if (n && n.trim() && n.trim() !== p.name) {
        runInNewTab(`rideshell profile rename ${p.name} ${n.trim()}`, { title: 'Rename account', icon: 'user' });
        setTimeout(() => post({ t: 'state' }), 2500);
      }
    } });
    items.push({ icon: 'doc', label: 'Details', run: () => runInNewTab(`rideshell profile show ${p.name}`, { profile: p.name, title: p.name, icon: 'user' }) });
    items.push({ icon: 'close', label: 'Delete account…', run: () => {
      if (confirm(`Delete the account ${p.name}? Its settings and keys in RideShell are removed. Tool sign-in folders stay on disk.`)) {
        runInNewTab(`rideshell profile delete ${p.name} --force`, { title: 'Delete account', icon: 'close' });
        setTimeout(() => post({ t: 'state' }), 2500);
      }
    } });
    menu(items, x, y);
  }

  // ----- launchers -------------------------------------------------------------

  function pinnedPath(t) { return t.path || tools.list.find((x) => x.name.toLowerCase() === t.name.toLowerCase())?.path || null; }

  function renderApps() {
    const box = $('#apps');
    box.innerHTML = '';
    if (!RS.state) return;
    const v = SET().values;
    for (const a of RS.state.apps.filter((a) => !v.hiddenApps.includes(a.id))) {
      const b = el('button', 'app' + (a.available ? '' : ' unavailable'));
      b.innerHTML = `${exeIcon(a.icon, a.id)}<span class="a-name">${esc(a.name)}</span>`;
      b.title = a.available ? `${a.name} · as ${activeProfile() || 'the default account'}` : `${a.name} is not installed${a.install ? ' · click to install' : ''}`;
      b.onclick = (ev) => launchApp(a, ev);
      b.oncontextmenu = (e) => { e.preventDefault(); menu([
        { icon: 'terminal', label: `Open ${a.name}`, run: () => launchApp(a) },
        { icon: 'close', label: 'Remove from sidebar', run: () => { v.hiddenApps.push(a.id); SET().save(); renderApps(); } },
        { icon: 'gear', label: 'Customize sidebar…', run: () => SET().open('sidebar') },
      ], e.clientX, e.clientY); };
      box.appendChild(b);
    }
    for (const t of v.pinned) {
      const b = el('button', 'app');
      b.innerHTML = `${exeIcon(pinnedPath(t), t.name)}<span class="a-name">${esc(t.name)}</span>`;
      b.title = `${t.name} · as ${activeProfile() || 'the default account'}`;
      b.onclick = () => openTab({ profile: activeProfile(), cwd: activeTab()?.live.cwd || null, command: t.name, title: t.name });
      b.oncontextmenu = (e) => { e.preventDefault(); menu([
        { icon: 'close', label: 'Unpin', run: () => { v.pinned = v.pinned.filter((x) => x.name !== t.name); SET().save(); renderApps(); } },
        { icon: 'gear', label: 'Customize sidebar…', run: () => SET().open('sidebar') },
      ], e.clientX, e.clientY); };
      box.appendChild(b);
    }
    const add = el('button', 'app add', `<span class="aicon plain">${I('plus')}</span><span class="a-name">Add</span>`);
    add.title = 'Pin installed tools to the sidebar';
    add.onclick = () => openTools();
    box.appendChild(add);
  }

  function offerInstall(a, x, y) {
    if (!a.install) { toast(`${a.name} is not installed on this computer.`, 'error'); return; }
    menu([
      { icon: 'external', label: `Install ${a.name}`, run: () => runInNewTab(a.install, { title: `Install · ${a.name}` }) },
      { icon: 'terminal', label: `Show the command: ${a.install}`, run: () => { navigator.clipboard.writeText(a.install).catch(() => {}); toast('Install command copied', 'ok'); } },
    ], x, y);
  }

  function launchApp(a, ev) {
    if (!a.available) {
      const r = ev?.currentTarget?.getBoundingClientRect?.();
      offerInstall(a, r ? r.left : innerWidth / 3, r ? r.bottom + 4 : innerHeight / 3);
      return;
    }
    const t = activeTab();
    openTab({ profile: activeProfile(), cwd: t?.live.cwd || null, command: a.command, title: a.name });
  }

  // ----- installed tools -------------------------------------------------------------

  const tools = { list: [], all: false, source: null, loaded: false };

  function onTools(m) {
    tools.list = m.tools; tools.all = m.all; tools.loaded = true;
    if (!m.all) $('#tools-count').textContent = m.tools.length;
    if (!$('#tools').hidden) renderTools();
  }

  function isPinned(name) { return SET().values.pinned.some((x) => x.name.toLowerCase() === name.toLowerCase()); }

  function openTools() {
    $('#tools').hidden = false;
    $('#tools-input').value = '';
    renderTools();
    post({ t: 'tools', all: $('#tools-all').checked });
    setTimeout(() => $('#tools-input').focus(), 20);
  }
  function closeTools() { $('#tools').hidden = true; activeTab()?.term.focus(); }

  function renderTools() {
    const q = $('#tools-input').value.trim().toLowerCase();
    const counts = {};
    for (const t of tools.list) counts[t.source] = (counts[t.source] || 0) + 1;
    const chips = $('#tools-chips');
    chips.innerHTML = '';
    const chip = (label, src, n) => {
      const c = el('button', 'fchip' + (tools.source === src ? ' on' : ''), `${esc(label)}${n != null ? `<b>${n}</b>` : ''}`);
      c.onclick = () => { tools.source = tools.source === src ? null : src; renderTools(); };
      chips.appendChild(c);
    };
    chip('All', null, tools.list.length);
    chip('Routed', '__routed', tools.list.filter((t) => t.shimmed).length);
    for (const [src, n] of Object.entries(counts).sort((a, b) => b[1] - a[1])) chip(src, src, n);

    let rows = tools.list;
    if (tools.source === '__routed') rows = rows.filter((t) => t.shimmed);
    else if (tools.source) rows = rows.filter((t) => t.source === tools.source);
    if (q) rows = rows.filter((t) => t.name.toLowerCase().includes(q)).sort((a, b) => a.name.toLowerCase().indexOf(q) - b.name.toLowerCase().indexOf(q));

    const list = $('#tools-list');
    list.innerHTML = '';
    if (!tools.loaded) { list.appendChild(el('div', 'p-group', 'Scanning…')); return; }
    const frag = document.createDocumentFragment();
    for (const t of rows.slice(0, 600)) {
      const r = el('div', 'trow');
      r.innerHTML = `${exeIcon(t.path, t.name, 'small')}
        <span class="tname" title="${esc(t.name)}">${esc(t.name)}<em>${esc(t.source)}</em></span>
        <span class="tpath" title="${esc(t.path)}">${esc(t.path)}</span>
        <button class="pin${isPinned(t.name) ? ' on' : ''}" title="${isPinned(t.name) ? 'Unpin from sidebar' : 'Pin to sidebar'}">${isPinned(t.name) ? '★' : '☆'}</button>
        <span class="switch${t.shimmed ? ' on' : ''}" title="Route through RideShell: runs with the active account's keys and settings in every terminal"><i></i>${t.shimmed ? 'Routed' : 'Route'}</span>
        <button class="run">Run</button>`;
      r.querySelector('.run').onclick = () => { closeTools(); openTab({ profile: activeProfile(), cwd: activeTab()?.live.cwd || null, command: t.name, title: t.name }); };
      r.querySelector('.pin').onclick = () => {
        const v = SET().values;
        if (isPinned(t.name)) v.pinned = v.pinned.filter((x) => x.name.toLowerCase() !== t.name.toLowerCase());
        else v.pinned.push({ name: t.name, source: t.source, path: t.path });
        SET().save(); renderApps(); renderTools();
      };
      r.querySelector('.switch').onclick = () => post({ t: 'shim', name: t.name, on: !t.shimmed });
      frag.appendChild(r);
    }
    list.appendChild(frag);
    $('#tools-foot').textContent = `${rows.length} of ${tools.list.length} tools${rows.length > 600 ? ' (first 600 shown, refine the search)' : ''} · Run opens a tab with the active account · Route makes a tool use RideShell accounts everywhere`;
  }

  $('#tools-input').addEventListener('input', renderTools);
  $('#tools-input').addEventListener('keydown', (e) => { if (e.key === 'Escape') closeTools(); });
  $('#tools-all').addEventListener('change', () => { tools.loaded = false; renderTools(); post({ t: 'tools', all: $('#tools-all').checked }); });
  $('#tools').addEventListener('mousedown', (e) => { if (e.target.id === 'tools') closeTools(); });

  // ----- status bar -------------------------------------------------------------

  function renderStatus() {
    const t = activeTab();
    const p = activeProfile();
    const chip = $('#sb-profile');
    if (p) { chip.innerHTML = `<i style="--c:${colorOf(p).solid}"></i>${esc(p)}`; chip.hidden = false; } else chip.hidden = true;
    const cwd = t?.live.cwd;
    $('#sb-cwd').innerHTML = cwd ? `${I('folder')}${esc(cwd)}<button class="sb-pin${isPinnedFolder(cwd) ? ' on' : ''}" title="${isPinnedFolder(cwd) ? 'Unpin this folder' : 'Pin this folder'}">${isPinnedFolder(cwd) ? '★' : '☆'}</button>` : '';
    $('#sb-cwd .sb-pin')?.addEventListener('click', () => togglePinFolder(cwd));
  }

  // ----- folders -------------------------------------------------------------

  const norm = (p) => String(p || '').replace(/[\\/]+$/, '').toLowerCase();
  const baseName = (p) => String(p || '').replace(/[\\/]+$/, '').split(/[\\/]/).pop() || p;
  const psQuote = (p) => `'${String(p).replace(/'/g, "''")}'`;
  function isPinnedFolder(p) { return (SET()?.values.pinnedFolders || []).some((x) => norm(x) === norm(p)); }
  function togglePinFolder(p) {
    const v = SET().values;
    v.pinnedFolders = isPinnedFolder(p) ? v.pinnedFolders.filter((x) => norm(x) !== norm(p)) : [...(v.pinnedFolders || []), p];
    SET().save(); renderStatus(); renderFiles(); renderFavorites();
  }
  let recentTimer = null;
  function rememberFolder(p) {
    if (!p || !SET()) return;
    const v = SET().values;
    v.recentFolders = [p, ...(v.recentFolders || []).filter((x) => norm(x) !== norm(p))].slice(0, 8);
    clearTimeout(recentTimer);
    recentTimer = setTimeout(() => SET().save(), 1500);
    if (!$('#pane-files').hidden) renderFolderLists();
  }

  // Go to a folder: `cd` in the current tab when it is at a prompt, else a new tab.
  function goFolder(path, { newTab = false } = {}) {
    const t = activeTab();
    if (newTab || !t || t.ended || !t.atPrompt) { openTab({ cwd: path }); return; }
    post({ t: 'input', id: t.id, data: '\x1b' + `Set-Location -LiteralPath ${psQuote(path)}\r` });
    t.atPrompt = false;
    t.term.focus();
  }

  function pickFolder() { post({ t: 'pickFolder', start: activeTab()?.live.cwd || RS.state?.home || null }); }

  function onDropped(items) {
    const dirs = items.filter((i) => i.dir), dropped = items.filter((i) => !i.dir);
    for (const d of dirs) openTab({ cwd: d.path });
    const t = activeTab();
    if (dropped.length && t && !t.ended) t.term.paste(dropped.map((f) => psQuote(f.path)).join(' '));
  }

  const files = { path: null, entries: [], parent: null, error: null };
  function followTab() {
    const cwd = activeTab()?.live.cwd;
    if (cwd && !$('#pane-files').hidden && norm(cwd) !== norm(files.path)) post({ t: 'listDir', path: cwd });
  }
  function onDir(m) {
    files.path = m.path; files.parent = m.parent || null; files.entries = m.entries || []; files.error = m.error || null;
    renderDir();
  }

  function folderRow(path, { name, icon = 'folder', dot = null, title = '' } = {}) {
    const r = el('div', 'frow');
    r.innerHTML = `<span class="fi">${I(icon)}</span><span class="fname">${esc(name || baseName(path))}</span>${dot ? `<i class="fdot" style="background:${dot}" title="${esc(title)}"></i>` : ''}<button class="fopen" title="Open in a new tab">${I('external')}</button>`;
    r.title = path + (title ? ` · ${title}` : '');
    r.onclick = (e) => { if (e.target.closest('.fopen')) goFolder(path, { newTab: true }); else goFolder(path); };
    r.oncontextmenu = (e) => { e.preventDefault(); menu([
      { icon: 'folder', label: 'Go here in this tab', run: () => goFolder(path) },
      { icon: 'plus', label: 'Open in a new tab', run: () => goFolder(path, { newTab: true }) },
      { icon: 'star', label: isPinnedFolder(path) ? 'Unpin folder' : 'Pin folder', run: () => togglePinFolder(path) },
    ], e.clientX, e.clientY); };
    return r;
  }

  function renderFavorites() {
    const favs = SET()?.values.pinnedFolders || [];
    for (const box of [$('#pinned-folders'), $('#fav-home')]) {
      box.innerHTML = '';
      for (const p of favs) box.appendChild(folderRow(p, { icon: 'star' }));
      if (!favs.length) box.appendChild(el('div', 'fempty', 'Star a folder to keep it here: ☆ next to the folder in the status bar, or right-click a folder.'));
    }
  }

  // Collapsible sidebar sections (remembered).
  function applyCollapsed() {
    const c = SET().values.collapsed || {};
    document.querySelectorAll('.sec').forEach((sec) => sec.classList.toggle('collapsed', !!c[sec.dataset.sec]));
  }
  document.querySelectorAll('.sec-toggle').forEach((btn) => {
    btn.querySelector('.chev').innerHTML = I('chevron');
    btn.onclick = () => {
      const sec = btn.closest('.sec');
      const v = SET().values;
      v.collapsed = { ...(v.collapsed || {}), [sec.dataset.sec]: !sec.classList.contains('collapsed') };
      sec.classList.toggle('collapsed');
      SET().save();
    };
  });

  function renderFolderLists() {
    const places = $('#places');
    places.innerHTML = '';
    for (const p of RS.state?.places || []) {
      places.appendChild(folderRow(p.path, { name: p.name, icon: p.kind === 'home' ? 'user' : 'folder', dot: p.profile ? colorOf(p.profile).solid : null, title: p.profile ? `opens as ${p.profile}` : '' }));
    }
    renderFavorites();
    const v = SET().values;
    // Recently worked: folders used in RideShell, plus VS Code / Cursor / Claude Code / Codex.
    const rec = $('#recent-folders');
    rec.innerHTML = '';
    const seenF = new Set();
    const merged = [];
    for (const p of (v.recentFolders || []).slice(0, 4)) merged.push({ path: p, source: 'RideShell' });
    for (const f of RS.worked || []) merged.push(f);
    for (const f of merged) {
      const k = norm(f.path);
      if (seenF.has(k)) continue;
      seenF.add(k);
      const r = folderRow(f.path, { title: `last used in ${f.source}` });
      r.querySelector('.fname').insertAdjacentHTML('afterend', `<em class="fsrc">${esc(f.source)}</em>`);
      rec.appendChild(r);
      if (seenF.size >= 10) break;
    }
    $('#recent-sec').hidden = !seenF.size;
  }

  function renderDir() {
    const crumbs = $('#crumbs'), list = $('#dirlist');
    crumbs.innerHTML = ''; list.innerHTML = '';
    if (!files.path) { list.appendChild(el('div', 'fempty', 'Open a tab to browse its folder.')); return; }
    const parts = files.path.split(/[\\/]/).filter(Boolean);
    let acc = '';
    parts.forEach((part, i) => {
      acc = i === 0 ? (part.endsWith(':') ? part + '\\' : '/' + part) : acc.replace(/[\\/]$/, '') + '\\' + part;
      const target = acc;
      const c = el('button', 'crumb', esc(part));
      c.onclick = () => post({ t: 'listDir', path: target });
      crumbs.appendChild(c);
      if (i < parts.length - 1) crumbs.appendChild(el('span', 'csep', '›'));
    });
    const here = activeTab()?.live.cwd;
    if (here && norm(here) !== norm(files.path)) {
      const go = el('button', 'fgo', `${I('folder')}<span>Go here in this tab</span>`);
      go.onclick = () => goFolder(files.path);
      list.appendChild(go);
    }
    if (files.error) { list.appendChild(el('div', 'fempty', esc(files.error))); return; }
    if (files.parent) {
      const up = el('div', 'frow up', `<span class="fi">${I('chevron')}</span><span class="fname">..</span>`);
      up.onclick = () => post({ t: 'listDir', path: files.parent });
      list.appendChild(up);
    }
    if (!files.entries.length) list.appendChild(el('div', 'fempty', 'Empty folder'));
    for (const e of files.entries) {
      const full = files.path.replace(/[\\/]$/, '') + '\\' + e.name;
      const r = el('div', 'frow' + (e.dir ? '' : ' file'), `<span class="fi">${I(e.dir ? 'folder' : 'doc')}</span><span class="fname">${esc(e.name)}</span>${e.dir ? `<button class="fcd" title="Go here in this tab">cd</button><button class="fopen" title="Open in a new tab">${I('external')}</button>` : ''}`);
      r.title = e.dir ? 'Click to browse · cd: go there in this tab' : 'Click to insert the path at the prompt';
      r.onclick = (ev) => {
        if (!e.dir) { const t = activeTab(); if (t && !t.ended) { t.term.paste(psQuote(full) + ' '); t.term.focus(); } return; }
        if (ev.target.closest('.fopen')) goFolder(full, { newTab: true });
        else if (ev.target.closest('.fcd')) goFolder(full);
        else post({ t: 'listDir', path: full });
      };
      r.ondblclick = () => { if (e.dir) goFolder(full); };
      list.appendChild(r);
    }
  }

  function renderFiles() {
    if ($('#pane-files').hidden) return;
    renderFolderLists();
    renderDir();
  }

  function showPane(name) {
    document.querySelectorAll('.side-tabs button').forEach((b) => b.classList.toggle('on', b.dataset.pane === name));
    $('#pane-home').hidden = name !== 'home';
    $('#pane-files').hidden = name !== 'files';
    if (name === 'files') {
      post({ t: 'worked' });
      renderFolderLists();
      const cwd = activeTab()?.live.cwd || RS.state?.home;
      if (cwd) post({ t: 'listDir', path: cwd }); else renderDir();
    }
  }

  // ----- menus, toasts, modal -------------------------------------------------------------

  function menu(items, x, y) {
    const m = $('#menu');
    m.innerHTML = '';
    for (const it of items) {
      if (it === '-') { m.appendChild(el('hr')); continue; }
      const b = el('button', '', `${it.html || I(it.icon)}<span>${esc(it.label)}</span>`);
      b.onclick = () => { hideMenu(); it.run(); };
      m.appendChild(b);
    }
    m.hidden = false;
    const r = m.getBoundingClientRect();
    m.style.left = Math.min(x, innerWidth - r.width - 8) + 'px';
    m.style.top = Math.min(y, innerHeight - r.height - 8) + 'px';
  }
  function hideMenu() { $('#menu').hidden = true; }
  document.addEventListener('mousedown', (e) => { if (!e.target.closest('#menu')) hideMenu(); });

  function toast(text, kind = '') {
    const t = el('div', 'toast ' + kind);
    t.textContent = text;
    $('#toasts').appendChild(t);
    setTimeout(() => t.remove(), kind === 'error' ? 7000 : 3500);
  }

  function showProfileModal() {
    $('#modal').hidden = false;
    $('#pf-name').value = ''; $('#pf-desc').value = '';
    setTimeout(() => $('#pf-name').focus(), 30);
  }
  $('#profile-form').addEventListener('submit', (e) => {
    e.preventDefault();
    post({ t: 'createProfile', name: $('#pf-name').value, description: $('#pf-desc').value });
    $('#modal').hidden = true;
  });
  $('#pf-cancel').onclick = () => { $('#modal').hidden = true; activeTab()?.term.focus(); };

  // ----- command palette -------------------------------------------------------------

  let pal = { items: [], sel: 0 };
  function paletteActions() {
    const acts = [];
    const add = (group, icon, label, run, hint = '') => acts.push({ group, icon, label, run, hint });
    add('General', 'plus', 'New tab', () => openTab({ profile: activeProfile() }), 'Ctrl+T');
    if (activeTab()) add('General', 'close', 'Close tab', () => closeTab(RS.active), 'Ctrl+W');
    add('General', 'sidebar', 'Toggle sidebar', toggleSidebar, 'Ctrl+B');
    for (const p of RS.state?.profiles || []) {
      add('Accounts', 'user', `Switch to ${p.name}`, () => switchAccount(p.name));
      add('Accounts', 'plus', `New tab as ${p.name}`, () => openTab({ profile: p.name }));
    }
    add('Accounts', 'plus', 'New account…', showProfileModal);
    for (const a of (RS.state?.apps || []).filter((a) => a.available)) acts.push({ group: 'Apps', html: exeIcon(a.icon, a.id, 'tiny'), label: `Open ${a.name}`, run: () => launchApp(a) });
    add('Folders', 'folder', 'Open folder…', pickFolder);
    for (const f of [...(SET().values.pinnedFolders || []), ...(SET().values.recentFolders || [])].filter((f, i, arr) => arr.findIndex((x) => norm(x) === norm(f)) === i).slice(0, 8)) {
      add('Folders', 'folder', `Open ${baseName(f)}`, () => goFolder(f, { newTab: true }), f);
    }
    const cur = activeProfile();
    const cp = (RS.state?.profiles || []).find((p) => p.name === cur);
    for (const t of (cp?.tools || []).filter((t) => t.login)) {
      acts.push({ group: 'Accounts', html: toolIcon(t, 'tiny'), label: `Sign ${cur} in to ${t.name}`, run: () => signIn(cur, t.id, t.name) });
    }
    if (cp) add('Accounts', 'plus', `Connect a tool to ${cur}…`, () => openConnect(cp));
    if (!RS.state?.elevated) add('System', 'shield', 'Open as Administrator', openAdmin);
    add('General', 'terminal', 'Browse installed tools', openTools);
    if (q2().length >= 2) for (const t of tools.list.filter((t) => t.name.toLowerCase().startsWith(q2().toLowerCase())).slice(0, 8)) {
      acts.push({ group: 'Tools', html: exeIcon(t.path, t.name, 'tiny'), label: `Run ${t.name}`, run: () => openTab({ profile: activeProfile(), cwd: activeTab()?.live.cwd || null, command: t.name, title: t.name }), hint: t.source });
    }
    add('System', 'gear', 'Settings', () => SET().open(), 'Ctrl+,');
    add('System', 'sparkle', 'Change theme…', () => SET().open('appearance'));
    if (q2()) for (const sc of SET().schemes()) add('Themes', 'sparkle', `Theme: ${sc.name}`, () => SET().set('theme', sc.name));
    add('System', 'folder', 'Open data folder', () => post({ t: 'open', what: 'data' }));
    add('System', 'pulse', 'Run diagnostics (rideshell doctor)', () => runInNewTab('rideshell doctor', { title: 'Doctor', icon: 'pulse' }));
    add('System', 'bolt', 'Zoom in', () => zoom(1), 'Ctrl+=');
    add('System', 'bolt', 'Zoom out', () => zoom(-1), 'Ctrl+-');
    add('System', 'external', 'RideShell on GitHub', () => post({ t: 'open', what: 'https://github.com/gauravvvvvvvvvv/RideShell' }));
    return acts;
  }

  function q2() { return $('#palette-input').value.trim(); }

  function fuzzy(q, s) {
    if (!q) return 1;
    q = q.toLowerCase(); s = s.toLowerCase();
    if (s.includes(q)) return 100 - s.indexOf(q);
    let i = 0, score = 0;
    for (const c of s) { if (c === q[i]) { i++; score++; } if (i === q.length) return score; }
    return 0;
  }

  function openPalette(initial = '') {
    $('#palette').hidden = false;
    const input = $('#palette-input');
    input.value = initial;
    pal.sel = 0;
    renderPalette();
    setTimeout(() => input.focus(), 10);
  }
  function closePalette() { $('#palette').hidden = true; activeTab()?.term.focus(); }
  function renderPalette() {
    const q = $('#palette-input').value.trim();
    const all = paletteActions();
    pal.items = all.map((a) => ({ a, s: fuzzy(q, a.label + ' ' + a.group) })).filter((x) => x.s > 0).sort((x, y) => (q ? y.s - x.s : 0)).map((x) => x.a);
    pal.sel = Math.min(pal.sel, Math.max(0, pal.items.length - 1));
    const list = $('#palette-list');
    list.innerHTML = '';
    let group = null;
    pal.items.forEach((a, i) => {
      if (!q && a.group !== group) { group = a.group; list.appendChild(el('div', 'p-group', esc(group))); }
      const it = el('div', 'p-item' + (i === pal.sel ? ' sel' : ''), `<span class="p-ico">${a.html || I(a.icon)}</span><span class="p-label">${esc(a.label)}</span><span class="p-hint">${esc(a.hint || '')}</span>`);
      it.onmouseenter = () => { pal.sel = i; for (const n of list.querySelectorAll('.p-item')) n.classList.remove('sel'); it.classList.add('sel'); };
      it.onclick = () => { closePalette(); a.run(); };
      list.appendChild(it);
    });
    if (!pal.items.length) list.appendChild(el('div', 'p-group', 'No matches'));
    list.querySelector('.sel')?.scrollIntoView({ block: 'nearest' });
  }
  $('#palette-input').addEventListener('input', () => { pal.sel = 0; renderPalette(); });
  $('#palette-input').addEventListener('keydown', (e) => {
    if (e.key === 'ArrowDown') { pal.sel = Math.min(pal.sel + 1, pal.items.length - 1); renderPalette(); e.preventDefault(); }
    else if (e.key === 'ArrowUp') { pal.sel = Math.max(pal.sel - 1, 0); renderPalette(); e.preventDefault(); }
    else if (e.key === 'Enter') { const a = pal.items[pal.sel]; closePalette(); if (a) a.run(); }
    else if (e.key === 'Escape') closePalette();
  });
  $('#palette').addEventListener('mousedown', (e) => { if (e.target.id === 'palette') closePalette(); });

  // ----- keyboard -------------------------------------------------------------

  function shortcut(e) {
    const ctrl = e.ctrlKey && !e.altKey && !e.metaKey;
    if (!ctrl) return null;
    const k = e.key.toLowerCase();
    if (k === 't' && !e.shiftKey) return () => openTab({ profile: activeProfile() });
    if (k === 'w' && !e.shiftKey) return () => RS.active != null && closeTab(RS.active);
    if (k === 'tab') return () => cycle(e.shiftKey ? -1 : 1);
    if ((k === 'k' && !e.shiftKey) || (k === 'p' && e.shiftKey)) return () => openPalette();
    if (k === 'b' && !e.shiftKey) return toggleSidebar;
    if (k === 'e' && e.shiftKey) return openTools;
    if (k === ',' && !e.shiftKey) return () => SET().open();
    if (k === '=' || k === '+') return () => zoom(1);
    if (k === '-') return () => zoom(-1);
    if (k === '0') return () => zoom(0);
    if (/^[1-9]$/.test(k) && !e.shiftKey) return () => { const id = RS.order[Number(k) - 1]; if (id != null) activate(id); };
    return null;
  }

  function keyHandler(e, tab) {
    if (e.type !== 'keydown') return true;
    const act = shortcut(e);
    if (act) { e.preventDefault(); act(); return false; }
    const k = e.key.toLowerCase();
    if (e.ctrlKey && k === 'c' && (e.shiftKey || tab.term.hasSelection())) {
      if (tab.term.hasSelection()) { navigator.clipboard.writeText(tab.term.getSelection()).catch(() => {}); if (!e.shiftKey) tab.term.clearSelection(); }
      e.preventDefault();
      return false;
    }
    // Ctrl+V: let the web view's paste event deliver the text (bracketed).
    if (e.ctrlKey && k === 'v') return false;
    return true;
  }

  document.addEventListener('keydown', (e) => {
    if (e.target.closest && e.target.closest('.xterm')) return;
    if (e.key === 'Escape') { hideMenu(); if (!$('#signin').hidden) { $('#signin').hidden = true; signing = null; } if (!$('#modal').hidden) { $('#modal').hidden = true; } if (!$('#tools').hidden) closeTools(); }
    const act = shortcut(e);
    if (act && $('#palette').hidden && $('#modal').hidden && $('#tools').hidden && $('#settings').hidden && $('#signin').hidden ) { e.preventDefault(); act(); }
  });

  function zoom(step) {
    const cur = SET().values.fontSize;
    SET().set('fontSize', step === 0 ? 13.5 : Math.max(9, Math.min(28, cur + step)));
  }

  function toggleSidebar() {
    SET().set('sidebar', $('#app').classList.contains('no-sidebar'));
    setTimeout(() => activeTab()?.fit.fit(), 240);
  }

  function openAdmin() {
    const t = activeTab();
    post({ t: 'admin', profile: activeProfile(), cwd: t?.live.cwd || null });
  }

  // ----- window chrome -------------------------------------------------------------

  document.querySelectorAll('[data-win]').forEach((b) => {
    const op = b.dataset.win;
    b.innerHTML = I(op === 'minimize' ? 'min' : op === 'maximize' ? 'max' : 'close');
    b.onclick = () => { if (op === 'maximize') document.body.classList.toggle('maximized'); post({ t: 'win', op }); };
  });
  document.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    const dragZone = e.target.closest('[data-drag]');
    if (dragZone && !e.target.closest('.tab, button')) {
      if (e.detail === 2) { document.body.classList.toggle('maximized'); post({ t: 'win', op: 'maximize' }); }
      else post({ t: 'win', op: 'drag' });
    }
  });
  document.querySelectorAll('#resize-edges div').forEach((d) => d.addEventListener('mousedown', (e) => { if (e.button === 0) post({ t: 'win', op: 'resize', dir: d.dataset.dir }); }));

  // ----- static chrome -------------------------------------------------------------

  $('.brand .logo').innerHTML = I('logo');
  $('.empty-logo').innerHTML = I('logo');
  $('#toggle-sidebar').innerHTML = I('sidebar');
  $('#toggle-sidebar').onclick = toggleSidebar;
  $('#new-tab').innerHTML = I('plus');
  $('#new-tab').onclick = () => openTab({ profile: activeProfile() });
  $('#new-tab').oncontextmenu = (e) => {
    e.preventDefault();
    menu([
      { icon: 'folder', label: 'New tab in folder…', run: pickFolder },
      '-',
      ...(RS.state?.profiles || []).map((p) => ({ icon: 'user', label: `New tab as ${p.name}`, run: () => openTab({ profile: p.name }) })),
    ], e.clientX, e.clientY);
  };
  $('#palette-btn .i').innerHTML = I('search');
  $('#palette-btn').onclick = () => openPalette();
  $('#admin-badge').innerHTML = `${I('shield')}Administrator`;
  $('#add-profile').innerHTML = I('plus');
  $('#add-profile').onclick = showProfileModal;
  $('#admin-btn .i').innerHTML = I('shield');
  $('#admin-btn').onclick = openAdmin;
  $('#settings-btn .i').innerHTML = I('gear');
  $('#settings-btn').onclick = () => SET().open();
  $('#all-tools-btn .i').innerHTML = I('terminal');
  $('#all-tools-btn').onclick = openTools;
  $('#tools-close').innerHTML = I('close');
  $('#tools-close').onclick = closeTools;
  $('#tools .tools-search .i').innerHTML = I('search');
  $('#empty-new').onclick = () => openTab({ profile: activeProfile() });
  $('#empty-folder').onclick = pickFolder;
  $('#open-folder').innerHTML = `${I('folder')}<span>Open folder…</span>`;
  $('#open-folder').onclick = pickFolder;
  document.querySelectorAll('.side-tabs button').forEach((b) => { b.onclick = () => showPane(b.dataset.pane); });

  Object.assign(RS, {
    post, toast, openTab, pickFolder, openTools, openAdmin, renderApps, activeProfile,
    newProfile: showProfileModal,
    fitActive: () => activeTab()?.fit.fit(),
    focusActive: () => activeTab()?.term.focus(),
  });

  new ResizeObserver(() => activeTab()?.fit.fit()).observe($('#terminals'));
  window.addEventListener('focus', () => post({ t: 'state' }));
  setInterval(() => { if (document.hasFocus()) post({ t: 'state' }); }, 10000);

  // Fonts must be ready before xterm measures cells.
  Promise.all([document.fonts.load('13px "Geist Mono"'), document.fonts.load('600 13px "Geist Mono"'), document.fonts.load('13px "Geist"')])
    .catch(() => {})
    .finally(() => { post({ t: 'state' }); post({ t: 'tools', all: false }); });
})();
