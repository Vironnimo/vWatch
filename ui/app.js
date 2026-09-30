(function () {
  'use strict';

  const PALETTE = ['#e8870a', '#4fae7a', '#5b8def', '#c0674a', '#9a7bd0', '#d06a9c', '#3aa6b0', '#b3a23a', '#7f9c4a', '#c9853f'];
  const PHASE = { active: 'aktiv', cooling: 'kühlt ab', dormant: 'schläft', done: 'fertig', maintenance: 'Wartung' };
  const STATUS = { active: 'aktiv', parked: 'geparkt', done: 'fertig', archived: 'archiviert' };
  const KIND = { worktree: 'Worktree', branch: 'Branch', uncommitted: 'nicht committet', flagged: 'FLAGGED', todo: 'TODO', plan: 'Plan', other: 'Sonstiges' };
  const TYPE = { next: 'Nächster Schritt', feature: 'Feature', cleanup: 'Aufräumen', park: 'Parken' };
  const CI = { green: ['grün', 'good'], red: ['rot', 'bad'], running: ['läuft', ''], none: ['keins', ''], unknown: ['unbekannt', ''], skipped: ['—', ''] };
  const VERDICT = { park: ['parken', 'warn'], revive: ['zurückholen', 'good'], finish: ['fertig machen', 'good'], archive: ['archivieren', 'bad'] };
  const MOD_TITLES = {
    focus: 'Heute zuerst', waiting: 'Wartet auf dich', safety: 'Sicherheitslage', recent: 'Was passiert ist', patterns: 'Muster',
    ideas: 'Ideen aus den Projekten', synergies: 'Querverbindungen', newapps: 'Neue App-Ideen', park: 'Parken oder zurückholen?',
    pulse: 'Puls', voices: 'Die Projekte melden sich', kpis: 'Lage auf einen Blick', cards: 'Projekte',
  };

  let S = null;           // state from the app core
  let showRated = false;  // show suggestions the owner already rated away
  let openId = null;      // project whose drawer is open
  let panelOpen = false;  // settings panel
  let settings = null;    // autostart, agent access, paths
  let disc = null;        // project search: { target, root, candidates, checked:Set, busy }
  let confirmRemove = null;
  let lastVersion = null;
  let shown = false;

  // ---------- helpers ----------
  const $ = sel => document.querySelector(sel);
  const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const dayMs = 864e5;
  const todayDate = () => new Date(S.today + 'T12:00:00');
  const daysSince = d => Math.round((todayDate() - new Date(String(d).slice(0, 10) + 'T12:00:00')) / dayMs);
  const ago = d => { if (!d) return 'nie'; const n = daysSince(d); return n <= 0 ? 'heute' : n === 1 ? 'gestern' : `vor ${n} Tagen`; };
  const minutesAgo = iso => Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60000));
  const fmtDate = d => { const [y, m, dd] = String(d).slice(0, 10).split('-'); return `${dd}.${m}.${y}`; };
  const fmtShort = d => { const [, m, dd] = String(d).slice(0, 10).split('-'); return `${dd}.${m}.`; };
  const num = n => Number(n || 0).toLocaleString('de-DE');
  const hash = s => { let h = 0; for (const c of s) h = (h * 31 + c.charCodeAt(0)) >>> 0; return h; };
  const color = id => PALETTE[hash(id) % PALETTE.length];
  const dot = id => `<span class="dot" style="background:${color(id)}"></span>`;
  const byId = id => S.projects.find(p => p.id === id);
  const plink = id => { const p = byId(id); return p ? `<button class="plink" type="button" data-open="${esc(id)}">${dot(id)}${esc(p.name)}</button>` : esc(id); };
  const isNew = iso => !!(iso && S.previous_visit && iso > S.previous_visit);
  const phasePill = p => p.card ? `<span class="pill ${esc(p.card.phase)}">${esc(PHASE[p.card.phase] || p.card.phase)}</span>` : '<span class="pill unknown">ohne Karte</span>';

  const visible = () => S.projects.filter(p => p.status !== 'archived');
  const lastWork = p => (p.facts && p.facts.last_work) || (p.card_updated_at || '').slice(0, 10) || null;
  const byLastWork = list => [...list].sort((a, b) => (lastWork(b) || '').localeCompare(lastWork(a) || ''));

  function rated(id) {
    const f = S.feedback[id];
    if (!f) return null;
    if (f.value === 'later' && (Date.now() - Date.parse(f.at)) / dayMs >= S.snooze_days) return null;
    return f.value;
  }
  const hiddenByRating = id => !showRated && (rated(id) === 'down' || rated(id) === 'later');

  // ---------- facts-derived safety ----------
  function safety(p) {
    const f = p.facts || {};
    const d = f.dirty || {};
    const out = { remote: f.has_remote, unsaved: null, level: null, ahead: f.ahead || 0, ci: (f.ci || {}).state || 'unknown', leftovers: null };
    if (d.files) {
      const lines = (d.lines_added || 0) + (d.lines_removed || 0);
      const age = d.oldest_change ? daysSince(d.oldest_change) : 0;
      out.unsaved = `${num(d.files)} Dateien, +${num(d.lines_added)}/−${num(d.lines_removed)}${age > 0 ? `, seit ${age} ${age === 1 ? 'Tag' : 'Tagen'}` : ''}`;
      out.level = lines > 500 || age > 3 ? 'bad' : 'warn';
    }
    const unmerged = ((f.branches || {}).unmerged || []).length;
    const wts = (f.worktrees || {}).count || 0;
    const parts = [];
    if (unmerged) parts.push(`${unmerged} ungemergte ${unmerged === 1 ? 'Branch' : 'Branches'}`);
    if (wts) parts.push(`${wts} ${wts === 1 ? 'Worktree' : 'Worktrees'}${(f.worktrees || {}).prunable ? `, ${(f.worktrees || {}).prunable} tot` : ''}`);
    out.leftovers = parts.join(' · ') || null;
    return out;
  }

  // ---------- building blocks ----------
  const hiddenMods = () => new Set(S.prefs.hidden_modules || []);
  function mod(id, title, sub, body) {
    MOD_TITLES[id] = title;
    if (hiddenMods().has(id) || !body) return '';
    return `<section class="mod" data-mod="${id}"><div class="mod-head"><div><h2>${esc(title)}</h2>${sub ? `<div class="sub">${esc(sub)}</div>` : ''}</div><button class="hide" type="button" data-hide="${id}" title="Diesen Baustein ausblenden">ausblenden</button></div>${body}</section>`;
  }
  const dmod = (title, body, sub) => body ? `<section class="mod"><div class="mod-head"><div><h2>${esc(title)}</h2>${sub ? `<div class="sub">${esc(sub)}</div>` : ''}</div></div>${body}</section>` : '';

  function fbButtons(id, projectId, title) {
    const cur = rated(id);
    return `<span class="fb" data-item="${esc(id)}" data-project="${esc(projectId || '')}" data-title="${esc(title)}">${[['up', 'gut'], ['later', 'später'], ['down', 'Quatsch']].map(([v, l]) => `<button type="button" data-v="${v}" aria-pressed="${cur === v}">${l}</button>`).join('')}</span>`;
  }

  function spark(weekly, col, h = 38) {
    const w = 260, n = weekly.length, max = Math.max(1, ...weekly), pad = 3;
    const x = i => pad + (i * (w - 2 * pad)) / (n - 1);
    const y = v => h - pad - (v / max) * (h - 2 * pad);
    const pts = weekly.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`);
    const area = `M${x(0)},${h - pad} L${pts.join(' L')} L${x(n - 1)},${h - pad} Z`;
    return `<svg class="spark" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none" role="img" aria-label="Commits pro Woche über 26 Wochen, höchstens ${max}">
      <line x1="${pad}" x2="${w - pad}" y1="${h - pad}" y2="${h - pad}" stroke="var(--line)" stroke-width="1"/>
      <path d="${area}" fill="${col}" fill-opacity="0.18"/>
      <polyline points="${pts.join(' ')}" fill="none" stroke="${col}" stroke-width="1.6" vector-effect="non-scaling-stroke"/>
      <circle cx="${x(n - 1)}" cy="${y(weekly[n - 1])}" r="2.6" fill="${col}"/></svg>`;
  }
  const weekly = p => (p.facts && p.facts.commits && p.facts.commits.weekly) || new Array(26).fill(0);

  function ideaCard(s, projects, projectId) {
    if (hiddenByRating(s.id)) return '';
    return `<div class="idea"><div class="h"><div class="t">${s.severity ? `<span class="sev-${esc(s.severity)}">●</span> ` : ''}${esc(s.title)}</div>${s.effort ? `<span class="effort">${esc(s.effort)}</span>` : ''}</div>
      <div class="why">${esc(s.why || s.pitch)}</div>
      ${s.because ? `<div class="why"><b>Weil:</b> ${esc(s.because)}</div>` : ''}
      ${s.evidence ? `<details><summary class="more">Beleg</summary><div class="ev">${esc(s.evidence)}</div></details>` : ''}
      <div class="foot"><div style="display:flex;gap:10px;flex-wrap:wrap;align-items:center">${(projects || []).map(plink).join('')}${s.type ? `<span class="tag">${esc(TYPE[s.type] || s.type)}</span>` : ''}</div>${fbButtons(s.id, projectId, s.title)}</div></div>`;
  }

  function blockFocus() {
    const t = (S.portfolio && S.portfolio.focus) || [];
    if (!t.length) return '';
    return mod('focus', 'Heute zuerst', 'Über alle Projekte hinweg ausgewählt', `<div class="focus">${t.slice(0, 3).map((it, i) => `
      <div class="item"><div class="n">${i + 1}</div><div><div class="t">${esc(it.title)}</div>
      <div class="meta">${plink(it.project)}<span class="effort">${esc(it.effort)}</span></div>
      <div class="muted" style="font-size:14px;margin-top:4px">${esc(it.why)}</div></div></div>`).join('')}</div>`);
  }

  function blockWaiting(limitPer = 2) {
    const groups = visible().filter(p => p.card && p.card.waiting_on_you.length);
    if (!groups.length) return '';
    const total = groups.reduce((a, p) => a + p.card.waiting_on_you.length, 0);
    const li = w => `<div class="row"><div class="t">${esc(w.title)}</div>${w.source ? `<div class="d mono" style="font-size:12px">${esc(w.source)}</div>` : ''}</div>`;
    const body = groups.map(p => {
      const items = p.card.waiting_on_you, head = items.slice(0, limitPer), rest = items.slice(limitPer);
      return `<div class="group-h">${plink(p.id)}<span class="faint">${items.length} offen</span></div><div class="list">${head.map(li).join('')}${rest.length ? `<details><summary class="more">+ ${rest.length} weitere</summary>${rest.map(li).join('')}</details>` : ''}</div>`;
    }).join('');
    return mod('waiting', 'Wartet auf dich', `${total} Entscheidungen, die nur du treffen kannst`, body);
  }

  function blockSafety() {
    const rows = byLastWork(visible()).filter(p => p.facts && p.facts.is_git).map(p => {
      const s = safety(p);
      const unsaved = [s.unsaved ? `<span class="tag ${s.level}">${esc(s.unsaved)}</span>` : '', s.ahead ? `<span class="tag warn">${s.ahead} ungepusht</span>` : ''].filter(Boolean).join(' ');
      const ci = CI[s.ci] || CI.unknown;
      return `<tr><td>${plink(p.id)}</td>
        <td>${s.remote ? `<span class="tag good">${esc(p.facts.remotes.map(r => r.name).join(', '))}</span>` : '<span class="tag bad">kein Remote</span>'}</td>
        <td>${unsaved || '<span class="faint">sauber</span>'}</td>
        <td>${s.leftovers ? `<span class="faint">${esc(s.leftovers)}</span>` : ''}</td>
        <td>${ci[1] ? `<span class="tag ${ci[1]}">${ci[0]}</span>` : `<span class="faint">${ci[0]}</span>`}</td></tr>`;
    }).join('');
    if (!rows) return '';
    return mod('safety', 'Sicherheitslage', 'Was verloren ginge, wenn heute die Platte stirbt oder ein Agent falsch auscheckt', `<div class="tablewrap"><table class="safety"><thead><tr><th>Projekt</th><th>Backup</th><th>Ungesichert</th><th>Liegengeblieben</th><th>CI</th></tr></thead><tbody>${rows}</tbody></table></div>`);
  }

  function blockRecent() {
    const items = visible().flatMap(p => (p.card ? p.card.recent_changes : []).map(c => ({ ...c, project: p.id }))).filter(c => daysSince(c.date) <= 14).sort((a, b) => b.date.localeCompare(a.date));
    if (!items.length) return '';
    let last = '';
    const prevDay = S.previous_visit ? S.previous_visit.slice(0, 10) : null;
    const body = items.map(c => {
      const h = c.date !== last ? `<div class="day-h">${fmtShort(c.date)} · ${ago(c.date)}</div>` : '';
      last = c.date;
      const fresh = prevDay && c.date >= prevDay ? ' <span class="pill new">neu</span>' : '';
      return `${h}<div class="row"><div class="t">${esc(c.title)}${fresh}</div><div>${plink(c.project)}</div>${c.detail ? `<div class="d">${esc(c.detail)}</div>` : ''}</div>`;
    }).join('');
    return mod('recent', 'Was passiert ist', 'Die größeren Änderungen der letzten 14 Tage, als Themen statt Commits', `<div class="list">${body}</div>`);
  }

  function blockPulse() {
    const list = visible().filter(p => p.facts && p.facts.is_git);
    if (!list.length) return '';
    const shares = list.map(p => [p.id, p.facts.commits.last_30d]).sort((a, b) => b[1] - a[1]);
    const total = shares.reduce((a, [, v]) => a + v, 0) || 1;
    const bar = shares.map(([id, v]) => v ? `<span style="width:${(v / total * 100).toFixed(2)}%;background:${color(id)}" title="${esc(byId(id).name)}: ${v}"></span>` : '').join('');
    const legend = shares.map(([id, v]) => `<span>${dot(id)}${esc(byId(id).name)} <b class="mono">${num(v)}</b></span>`).join('');
    const rows = byLastWork(list).map(p => `<div class="spark-row"><div>${plink(p.id)}</div>${spark(weekly(p), color(p.id), 26)}<div class="last faint" style="font-size:12.5px">${ago(lastWork(p))}</div></div>`).join('');
    const note = (S.portfolio && S.portfolio.attention_note) || 'Commits der letzten 30 Tage und Verlauf über 26 Wochen';
    return mod('pulse', 'Puls', note, `<div class="attn-bar" role="img" aria-label="Anteil der Commits in 30 Tagen">${bar}</div><div class="attn-legend">${legend}</div><div style="margin-top:14px">${rows}</div><p class="faint" style="font-size:12.5px;margin-top:6px">Jede Kurve hat ihre eigene Skala. „zuletzt“ zählt auch nicht committete Arbeit.</p>`);
  }

  function blockProjectIdeas() {
    const cards = visible().map(p => {
      const list = ((p.card || {}).suggestions || []).filter(s => !hiddenByRating(s.id));
      const s = list.find(x => x.type === 'feature') || list[0];
      return s ? ideaCard(s, [p.id], p.id) : '';
    }).filter(Boolean);
    if (!cards.length) return '';
    return mod('ideas', 'Ideen aus den Projekten', 'Pro Projekt der interessanteste Vorschlag. Weitere stehen im Steckbrief', `<div class="cards2">${cards.join('')}</div>`);
  }

  function blockPortfolioList(id, key, title, sub) {
    const items = ((S.portfolio || {})[key] || []).map(s => ideaCard(s, s.projects || [], null)).filter(Boolean);
    return items.length ? mod(id, title, sub, `<div class="cards2">${items.join('')}</div>`) : '';
  }

  function blockPark() {
    const it = ((S.portfolio || {}).park_or_revive || []).filter(r => !hiddenByRating(r.id));
    if (!it.length) return '';
    return mod('park', 'Parken oder zurückholen?', 'Ehrliche Einschätzung zu den stillen Projekten', `<div class="list">${it.map(r => {
      const v = VERDICT[r.verdict] || [r.verdict, ''];
      return `<div class="row"><div>${plink(r.project)}</div><div><span class="tag ${v[1]}">${esc(v[0])}</span></div><div class="d">${esc(r.why)}</div><div style="grid-column:1/-1">${fbButtons(r.id, r.project, `${v[0]}: ${byId(r.project) ? byId(r.project).name : r.project}`)}</div></div>`;
    }).join('')}</div>`);
  }

  function blockVoices() {
    const list = visible().filter(p => p.card && p.card.personality);
    if (!list.length) return '';
    return mod('voices', 'Die Projekte melden sich', 'Ein Satz pro Projekt, aus seiner Sicht', `<div class="quotes">${list.map(p => `<div class="quote">${plink(p.id)}<p>„${esc(p.card.personality)}“</p></div>`).join('')}</div>`);
  }

  function missingCardsNote() {
    const missing = visible().filter(p => !p.card);
    if (!missing.length) return '';
    return `<div class="note" style="margin-top:24px">${missing.length === 1 ? 'Ein Projekt hat' : `${missing.length} Projekte haben`} noch keine Einschätzung: ${missing.map(p => plink(p.id)).join(', ')}. Der nächste Agent-Lauf schreibt sie. <button class="linkish" type="button" data-open-settings>Wie richte ich den Lauf ein?</button></div>`;
  }

  // ---------- handover ----------
  function renderHandover() {
    const n = visible().length;
    const greeting = (S.portfolio && S.portfolio.greeting) || `Guten Morgen. ${n} ${n === 1 ? 'Projekt' : 'Projekte'} im Blick.`;
    const fresh = isNew(S.portfolio_updated_at) ? '<span class="pill new">neu</span>' : '';
    return `<div class="handover">
      <div class="greet"><div class="kicker">Übergabe · ${n} ${n === 1 ? 'Projekt' : 'Projekte'} ${fresh}</div><h2>${esc(greeting)}</h2></div>
      ${missingCardsNote()}
      ${blockFocus()}${blockWaiting()}${blockSafety()}${blockRecent()}
      ${blockPortfolioList('patterns', 'patterns', 'Muster, die du vielleicht nicht siehst', 'Alle Projekte nebeneinander gelegt')}
      ${blockProjectIdeas()}
      ${blockPortfolioList('synergies', 'synergies', 'Querverbindungen', 'Was ein Projekt vom anderen übernehmen könnte')}
      ${blockPortfolioList('newapps', 'new_apps', 'Neue App-Ideen', 'Abgeleitet aus dem, was du baust und was dir offenbar fehlt')}
      ${blockPark()}${blockPulse()}${blockVoices()}
    </div>`;
  }

  // ---------- board ----------
  function card(p) {
    const s = safety(p), c = p.card;
    const badges = [
      p.facts && p.facts.is_git && !s.remote ? '<span class="tag bad">kein Remote</span>' : '',
      s.unsaved ? `<span class="tag ${s.level}">${num(p.facts.dirty.files)} Dateien offen</span>` : '',
      s.ahead ? `<span class="tag warn">${s.ahead} ungepusht</span>` : '',
      s.ci === 'red' ? '<span class="tag bad">CI rot</span>' : '',
      p.status !== 'active' ? `<span class="tag">${esc(STATUS[p.status])}</span>` : '',
      p.card_outdated ? '<span class="tag">seit Karte geändert</span>' : '',
    ].join('');
    const fresh = isNew(p.card_updated_at) ? '<span class="pill new">neu</span>' : '';
    const commits = (p.facts && p.facts.commits) || { last_30d: 0 };
    return `<button class="card" type="button" data-open="${esc(p.id)}">
      <div class="h"><span class="name">${dot(p.id)}${esc(p.name)}</span><span style="display:flex;gap:6px">${fresh}${phasePill(p)}</span></div>
      <div class="hl">${c ? esc(c.headline) : '<span class="faint">Noch keine Einschätzung. Kommt beim nächsten Agent-Lauf.</span>'}</div>
      ${spark(weekly(p), color(p.id))}
      <div class="stats"><span>zuletzt ${ago(lastWork(p))}</span><span class="mono">${num(commits.last_30d)} / 30 T</span></div>
      ${badges ? `<div class="badges">${badges}</div>` : ''}
      ${c ? `<div class="next"><span class="faint" style="font-size:11.5px;letter-spacing:.06em;text-transform:uppercase">Wiedereinstieg</span><b>${esc(c.reentry.next_step)}</b><span><span class="effort">${esc(c.reentry.effort)}</span> · ${c.waiting_on_you.length} Entscheidungen · ${c.open_ends.length} offene Enden</span></div>` : ''}
    </button>`;
  }

  function renderBoard() {
    const list = visible();
    const git = list.filter(p => p.facts && p.facts.is_git);
    const noRemote = git.filter(p => !p.facts.has_remote).length;
    const unsaved = git.filter(p => (p.facts.dirty || {}).files || p.facts.ahead).length;
    const red = git.filter(p => (p.facts.ci || {}).state === 'red').length;
    const waiting = list.reduce((a, p) => a + (p.card ? p.card.waiting_on_you.length : 0), 0);
    const kpis = `<div class="kpis">
      <div class="kpi ${noRemote ? 'bad' : 'ok'}"><div class="v">${noRemote}</div><div class="l">Repos ohne Backup</div></div>
      <div class="kpi ${unsaved ? 'warn' : 'ok'}"><div class="v">${unsaved}</div><div class="l">mit ungesicherter Arbeit</div></div>
      <div class="kpi ${red ? 'bad' : 'ok'}"><div class="v">${red}</div><div class="l">mit rotem CI</div></div>
      <div class="kpi"><div class="v">${waiting}</div><div class="l">Entscheidungen offen</div></div></div>`;
    const grid = mod('cards', 'Projekte', 'Sortiert nach letzter Arbeit. Klick öffnet den Steckbrief', `<div class="grid">${byLastWork(list).map(card).join('')}</div>`);
    return `${mod('kpis', 'Lage auf einen Blick', null, kpis)}
      <div class="board"><div>${grid}${blockPulse()}${blockRecent()}</div>
      <aside class="side">${blockFocus()}${blockPortfolioList('patterns', 'patterns', 'Muster, die du vielleicht nicht siehst', null)}${blockPortfolioList('newapps', 'new_apps', 'Neue App-Ideen', null)}${blockPark()}</aside></div>`;
  }

  // ---------- drawer ----------
  function drawerHtml(p) {
    const c = p.card, f = p.facts || {}, s = safety(p);
    const commits = f.commits || {};
    const statusBar = `<div class="statusbar"><span class="faint" style="font-size:13px">Deine Absicht:</span><span class="seg" role="group" aria-label="Status">${Object.entries(STATUS).map(([k, l]) => `<button type="button" data-status="${k}" aria-pressed="${p.status === k}">${l}</button>`).join('')}</span><input id="status-note" type="text" placeholder="Notiz, z. B. warum geparkt" value="${esc(p.status_note)}" aria-label="Notiz zum Status"></div>`;
    const factsKv = f.is_git ? `<dl class="kv">
      <dt>Commits</dt><dd class="mono">${num(commits.total)} gesamt · 7 T: ${num(commits.last_7d)} · 30 T: ${num(commits.last_30d)} · seit ${commits.first ? fmtDate(commits.first) : '—'}</dd>
      <dt>Branch</dt><dd class="mono">${esc(f.branch || '—')}${f.upstream ? ` → ${esc(f.upstream)}` : ''}${f.ahead ? ` · ${f.ahead} voraus` : ''}${f.behind ? ` · ${f.behind} zurück` : ''}</dd>
      <dt>Backup</dt><dd>${f.has_remote ? esc(f.remotes.map(r => `${r.name}: ${r.url}`).join(', ')) : '<span class="tag bad">kein Remote</span>'}</dd>
      <dt>Ungesichert</dt><dd>${s.unsaved ? `<span class="tag ${s.level}">${esc(s.unsaved)}</span>` : 'sauber'}</dd>
      <dt>Liegengeblieben</dt><dd>${esc(s.leftovers || 'nichts')}</dd>
      <dt>CI</dt><dd>${esc((CI[s.ci] || CI.unknown)[0])}${f.ci && f.ci.title ? ` · ${esc(f.ci.workflow || '')}: ${esc(f.ci.title)}` : ''}${f.ci && f.ci.url ? ` · <button class="linkish" type="button" data-external="${esc(f.ci.url)}">Lauf öffnen</button>` : ''}</dd>
      </dl>` : `<p class="muted">${f.error === 'path_missing' ? 'Der Ordner existiert nicht mehr.' : 'Kein Git-Repository, daher keine Git-Fakten.'}</p>`;
    let body = `<button class="close" data-close type="button">Schließen</button>
      <h2 class="pname">${dot(p.id)}${esc(p.name)} ${phasePill(p)}${isNew(p.card_updated_at) ? '<span class="pill new">neu</span>' : ''}</h2>
      ${c ? `<div class="one">${esc(c.one_liner)}</div>` : ''}
      <div class="stack">${c && c.stack ? `${esc(c.stack)} · ` : ''}<button class="linkish mono" type="button" data-external="${esc(p.path)}" title="Im Explorer öffnen">${esc(p.path)}</button></div>
      ${statusBar}
      ${c && c.personality ? `<div class="say">„${esc(c.personality)}“</div>` : ''}
      <div style="margin-top:14px">${spark(weekly(p), color(p.id), 48)}${c && c.phase_evidence ? `<div class="faint" style="font-size:12.5px">${esc(c.phase_evidence)}</div>` : ''}</div>`;
    if (!c) {
      body += `<div class="note" style="margin-top:20px">Noch keine Einschätzung. Der nächste Agent-Lauf schreibt sie. Bis dahin siehst du nur die Git-Fakten.</div>`;
    } else {
      const v = c.vision || {};
      const suggestions = c.suggestions.map(sg => ideaCard(sg, [], p.id)).filter(Boolean).join('');
      body += dmod('Wiedereinstieg', `<div class="reentry"><div class="t">${esc(c.reentry.next_step)}</div><div><span class="effort">${esc(c.reentry.effort)}</span></div>${c.reentry.why ? `<div class="muted" style="font-size:14px">${esc(c.reentry.why)}</div>` : ''}</div>`)
        + dmod('Wartet auf dich', c.waiting_on_you.length ? `<div class="list">${c.waiting_on_you.map(w => `<div class="row"><div class="t">${esc(w.title)}</div>${w.source ? `<div class="d mono" style="font-size:12px">${esc(w.source)}</div>` : ''}</div>`).join('')}</div>` : '')
        + dmod('Offene Enden', c.open_ends.length ? `<div class="list">${c.open_ends.map(o => `<div class="row"><div class="t">${esc(o.title)}</div><div style="display:flex;gap:6px;align-items:start"><span class="tag ${o.kind === 'uncommitted' ? 'bad' : ''}">${esc(KIND[o.kind] || o.kind)}</span>${o.age_days != null ? `<span class="faint mono" style="font-size:12px">${o.age_days} T</span>` : ''}</div>${o.detail ? `<div class="d">${esc(o.detail)}</div>` : ''}${o.source ? `<div class="d mono" style="font-size:12px">${esc(o.source)}</div>` : ''}</div>`).join('')}</div>` : '', 'Angefangenes, Liegengebliebenes, Ungesichertes')
        + dmod('Zuletzt passiert', c.recent_changes.length ? `<div class="list">${c.recent_changes.map(ch => `<div class="row"><div class="t">${esc(ch.title)}</div><div class="faint mono" style="font-size:12px">${fmtShort(ch.date)}</div>${ch.detail ? `<div class="d">${esc(ch.detail)}</div>` : ''}</div>`).join('')}</div>` : '')
        + dmod('Vorschläge', suggestions ? `<div class="cards2">${suggestions}</div>` : '')
        + dmod('Vision und Fortschritt', v.statement || v.progress_note ? `<div>${v.statement ? `<div>${esc(v.statement)}</div>` : ''}${v.progress_pct != null ? `<div class="prog"><span style="width:${Number(v.progress_pct)}%"></span></div><div class="faint mono" style="font-size:12px">${Number(v.progress_pct)} %</div>` : ''}${v.progress_note ? `<div class="muted" style="font-size:14px;margin-top:6px">${esc(v.progress_note)}</div>` : ''}</div>` : '')
        + dmod('Was es heute kann', c.features.length ? `<div class="feat">${c.features.map(x => `<span>${esc(x)}</span>`).join('')}</div>` : '')
        + dmod('Meilensteine', c.milestones.length ? `<ol class="timeline">${c.milestones.map(m => `<li><span class="when">${fmtDate(m.date)}</span><span class="rail"></span><span class="what">${esc(m.title)}</span></li>`).join('')}</ol>` : '')
        + dmod('Gesundheit', c.health_notes.length ? `<ul class="bullets">${c.health_notes.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : '')
        + dmod('Querverbindungen', c.cross_project_hints.length ? `<ul class="bullets">${c.cross_project_hints.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : '');
    }
    body += dmod('Fakten aus Git', factsKv, p.facts_collected_at ? `gemessen vor ${minutesAgo(p.facts_collected_at)} Min${p.card_updated_at ? ` · Karte vom ${fmtDate(p.card_updated_at)}` : ''}` : null);
    return `<div class="scrim" data-close></div><aside class="drawer" role="dialog" aria-modal="true" aria-label="${esc(p.name)}">${body}</aside>`;
  }

  function openDrawer(id, keepScroll) {
    const p = byId(id);
    if (!p) return;
    const root = $('#drawer-root');
    const prev = root.querySelector('.drawer');
    const top = keepScroll && prev ? prev.scrollTop : 0;
    root.innerHTML = drawerHtml(p);
    openId = id;
    const d = root.querySelector('.drawer');
    d.scrollTop = top;
    if (!keepScroll) root.querySelector('.close').focus();
  }
  const closeDrawer = () => { $('#drawer-root').innerHTML = ''; openId = null; };

  // ---------- page ----------
  function renderFoot() {
    const hidden = [...hiddenMods()];
    const ratedCount = Object.keys(S.feedback).filter(id => rated(id) === 'down' || rated(id) === 'later').length;
    const archived = S.projects.filter(p => p.status === 'archived');
    const parts = [];
    if (hidden.length) parts.push(`Ausgeblendet: ${hidden.map(id => `${esc(MOD_TITLES[id] || id)} <button type="button" data-unhide="${esc(id)}">einblenden</button>`).join(' · ')}`);
    if (ratedCount) parts.push(`${ratedCount} bewertete Vorschläge ${showRated ? 'werden angezeigt' : 'ausgeblendet'} <button type="button" data-toggle-rated>${showRated ? 'wieder ausblenden' : 'anzeigen'}</button>`);
    if (archived.length) parts.push(`Archiviert: ${archived.map(p => plink(p.id)).join(', ')}`);
    parts.push(`Karten schreiben Agents mit <code class="mono">lookout</code> · Fakten misst die App selbst`);
    $('#foot').innerHTML = parts.map(x => `<span>${x}</span>`).join('');
  }

  // ---------- adding projects ----------
  const RECENT_DAYS = 120;  // pre-select repositories with a commit in this window
  function discHtml(target) {
    if (!disc || disc.target !== target) return '';
    if (disc.busy) return `<p class="muted disc-msg">${esc(disc.busy)}</p>`;
    if (!disc.candidates.length) return `<p class="muted disc-msg">In <span class="mono">${esc(disc.root)}</span> liegen keine Git-Repos, die Lookout noch nicht kennt.</p>`;
    const n = disc.checked.size, all = n === disc.candidates.length;
    const rows = disc.candidates.map(c => `<label class="disc-row"><input type="checkbox" data-disc="${esc(c.path)}" ${disc.checked.has(c.path) ? 'checked' : ''}><span class="t">${esc(c.name)}</span><span class="faint">${c.last_commit ? `letzter Commit ${ago(c.last_commit)}` : 'noch kein Commit'}</span></label>`).join('');
    return `<div class="disc"><div class="disc-head"><span>${disc.candidates.length} Git-Repos in <span class="mono">${esc(disc.root)}</span></span><button class="linkish" type="button" data-disc-all>${all ? 'keins' : 'alle'} auswählen</button></div>
      <div class="disc-list">${rows}</div>
      <div class="actions"><button class="btn primary" type="button" data-disc-add ${n ? '' : 'disabled'}>${n === 1 ? '1 Projekt' : `${n} Projekte`} hinzufügen</button><button class="btn" type="button" data-disc-cancel>Abbrechen</button></div></div>`;
  }

  function renderEmpty() {
    return `<div class="empty"><div class="kicker">Willkommen</div><h2>Welche Projekte soll Lookout im Blick behalten?</h2>
      <p class="muted">Wähle den Ordner, in dem deine Projekte liegen. Lookout findet die Git-Repos darin und misst sofort, wie es um sie steht. Die Einschätzungen schreibt danach ein Agent; wie du den einrichtest, steht in den Einstellungen.</p>
      <div class="actions"><button class="btn primary" type="button" data-pick="folder" data-target="main">Projektordner wählen</button><button class="btn" type="button" data-pick="single" data-target="main">Einzelnes Projekt hinzufügen</button></div>
      ${discHtml('main')}</div>`;
  }

  // ---------- settings panel ----------
  function toggle(key, on, label, sub) {
    return `<div class="setting"><div><div class="t">${esc(label)}</div>${sub ? `<div class="d">${esc(sub)}</div>` : ''}</div><button class="toggle" type="button" role="switch" aria-checked="${on}" data-setting="${key}" aria-label="${esc(label)}"><span></span></button></div>`;
  }
  const copyLine = (text, label) => `<div class="copyline"><code class="mono">${esc(text)}</code><button class="btn" type="button" data-copy="${esc(text)}" aria-label="${esc(label)} kopieren">Kopieren</button></div>`;

  function panelHtml() {
    const st = settings || {};
    const projects = S.projects.map(p => `<div class="prow"><div class="pmain"><div class="pname-s">${dot(p.id)}<b>${esc(p.name)}</b>${p.status !== 'active' ? `<span class="tag">${esc(STATUS[p.status])}</span>` : ''}</div><div class="d mono">${esc(p.path)}</div></div>
      ${confirmRemove === p.id
        ? `<div class="confirm"><span>Karten und Verlauf werden gelöscht.</span><button class="btn danger" type="button" data-remove="${esc(p.id)}">Entfernen</button><button class="btn" type="button" data-remove-cancel>Behalten</button></div>`
        : `<button class="linkish" type="button" data-remove-ask="${esc(p.id)}">entfernen</button>`}</div>`).join('');
    return `<div class="scrim" data-close-panel></div><aside class="drawer panel" role="dialog" aria-modal="true" aria-label="Einstellungen">
      <button class="close" data-close-panel type="button">Schließen</button>
      <h2 class="pname">Einstellungen</h2>
      <section class="mod"><div class="mod-head"><div><h2>Projekte</h2><div class="sub">${S.projects.length ? `${S.projects.length} im Blick. Status und Notiz setzt du im Steckbrief eines Projekts.` : 'Noch keine.'}</div></div></div>
        <div class="plist">${projects}</div>
        <div class="actions"><button class="btn" type="button" data-pick="folder" data-target="panel">Ordner durchsuchen …</button><button class="btn" type="button" data-pick="single" data-target="panel">Einzelnes Projekt …</button></div>
        ${discHtml('panel')}</section>
      <section class="mod"><div class="mod-head"><div><h2>Start</h2></div></div>
        ${toggle('autostart', !!st.autostart, 'Beim Anmelden öffnen', 'Lookout geht auf, sobald du dich bei Windows anmeldest.')}</section>
      <section class="mod"><div class="mod-head"><div><h2>Agents</h2><div class="sub">Agents lesen deine Repos und schreiben die Einschätzungen mit dem Befehl „lookout“.</div></div></div>
        ${toggle('agent_access', !!st.agent_access, 'Für Agents aufrufbar', 'Trägt den Lookout-Ordner in deinen PATH ein. Programme, die schon laufen (etwa vBot), sehen das erst nach einem Neustart.')}
        <div class="setting-block"><div class="t">Skill für Agents</div><div class="d">Erklärt einem Agent den täglichen Lauf. Lookout hält ihn bei jedem Start aktuell.</div>${copyLine(st.skill_dir || '', 'Pfad')}</div>
        <div class="setting-block"><div class="t">In vBot installieren</div><div class="d">Danach in vBot einen täglichen Schedule anlegen, etwa „Führe den täglichen Lookout-Lauf durch.“</div>${copyLine(st.vbot_install || '', 'Befehl')}</div></section>
      <section class="mod"><div class="mod-head"><div><h2>Über</h2></div></div>
        <dl class="kv"><dt>Version</dt><dd class="mono">${esc(st.version || '')}</dd><dt>Programm</dt><dd class="mono">${esc(st.exe || '')}</dd><dt>Daten</dt><dd class="mono">${esc(st.data_dir || '')}</dd></dl></section>
    </aside>`;
  }
  function renderPanel(keepScroll) {
    const root = $('#panel-root');
    if (!panelOpen) { root.innerHTML = ''; return; }
    const prev = root.querySelector('.drawer');
    const top = keepScroll && prev ? prev.scrollTop : 0;
    root.innerHTML = panelHtml();
    root.querySelector('.drawer').scrollTop = top;
  }
  async function openPanel() {
    closeDrawer();
    panelOpen = true;
    renderPanel(false);
    try { settings = await call('settings'); renderPanel(true); } catch (e) { toast(errText(e)); }
  }
  function closePanel() {
    panelOpen = false;
    confirmRemove = null;
    if (disc && disc.target === 'panel') disc = null;
    renderPanel(false);
  }

  // ---------- page ----------
  function render() {
    const d = new Date(S.today + 'T12:00:00');
    $('#date').textContent = d.toLocaleDateString('de-DE', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
    const collected = S.projects.map(p => p.facts_collected_at).filter(Boolean).sort()[0];
    $('#fresh').textContent = collected ? `Fakten von vor ${minutesAgo(collected)} Min` : '';
    $('#refresh').hidden = $('.switch').hidden = !S.projects.length;
    document.querySelectorAll('.switch button').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.style === S.prefs.style)));
    $('#view').innerHTML = !S.projects.length ? renderEmpty() : S.prefs.style === 'board' ? renderBoard() : renderHandover();
    renderFoot();
    if (openId) openDrawer(openId, true);
    if (panelOpen) renderPanel(true);
  }

  // ---------- calls into the app ----------
  const call = (cmd, args) => window.__TAURI__.core.invoke(cmd, args || {});
  const errText = e => (e && e.message) || String(e);
  let toastTimer = null;
  function toast(text, undo) {
    const t = $('#toast');
    t.innerHTML = `<span>${esc(text)}</span>${undo ? '<button type="button" id="undo">Rückgängig</button>' : ''}`;
    t.hidden = false;
    if (undo) $('#undo').onclick = () => { t.hidden = true; undo(); };
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { t.hidden = true; }, 5000);
  }
  async function load(visit) {
    try { S = await call('state', { visit }); render(); }
    catch (e) { $('#view').innerHTML = `<p class="note" style="margin-top:30px">Lookout konnte seine Daten nicht laden: ${esc(errText(e))}</p>`; }
    if (!shown) { shown = true; call('ready').catch(() => {}); }
  }
  let refreshing = false;
  async function refreshFacts() {
    if (refreshing || !S || !S.projects.length) return;
    refreshing = true;
    const b = $('#refresh');
    b.disabled = true; b.textContent = 'Misst …';
    try { S = await call('refresh'); render(); } catch (e) { toast('Messen hat nicht geklappt: ' + errText(e)); }
    b.disabled = false; b.textContent = 'Aktualisieren';
    refreshing = false;
  }
  async function setFeedback(el, v) {
    const id = el.dataset.item, prev = S.feedback[id] ? S.feedback[id].value : null;
    const value = prev === v ? null : v;
    const send = val => call('set_feedback', { item_id: id, value: val, project_id: el.dataset.project || null, title: el.dataset.title });
    try {
      await send(value);
      if (value) S.feedback[id] = { value, at: new Date().toISOString() }; else delete S.feedback[id];
      render();
      if (value === 'down' || value === 'later') toast(value === 'down' ? 'Ausgeblendet. Der Agent schlägt das nicht mehr vor.' : `Kommt in ${S.snooze_days} Tagen wieder.`, async () => {
        await send(prev);
        if (prev) S.feedback[id] = { value: prev, at: new Date().toISOString() }; else delete S.feedback[id];
        render();
      });
    } catch (e) { toast('Speichern hat nicht geklappt.'); }
  }
  async function setPref(key, value) { S.prefs[key] = value; render(); try { await call('set_pref', { key, value }); } catch (e) { toast('Einstellung nicht gespeichert.'); } }
  async function setStatus(status, note) {
    try { await call('set_status', { id: openId, status, note }); await load(false); }
    catch (e) { toast('Status nicht gespeichert.'); }
  }

  function rerenderDisc() { if (disc && disc.target === 'panel') renderPanel(true); else render(); }
  async function pick(kind, target) {
    let path;
    try { path = await call('pick_folder'); } catch (e) { toast(errText(e)); return; }
    if (!path) return;
    if (kind === 'single') { await addPaths([path], target); return; }
    disc = { target, root: path, candidates: [], checked: new Set(), busy: 'Suche Git-Repos …' };
    rerenderDisc();
    try {
      const found = await call('discover', { root: path });
      const recent = found.filter(c => c.last_commit && daysSince(c.last_commit) <= RECENT_DAYS);
      disc = { target, root: path, candidates: found, checked: new Set((recent.length ? recent : found).map(c => c.path)), busy: null };
    } catch (e) { disc = null; toast(errText(e)); }
    rerenderDisc();
  }
  async function addPaths(paths, target) {
    disc = { ...(disc || { target, root: '', candidates: [], checked: new Set() }), busy: paths.length === 1 ? 'Füge hinzu und messe …' : `Füge ${paths.length} Projekte hinzu und messe …` };
    rerenderDisc();
    try {
      const res = await call('add_projects', { paths });
      disc = null;
      await load(false);
      if (res.failed.length) toast(res.failed.map(f => f.error.message).join(' '));
      else toast(res.added.length === 1 ? `${res.added[0].name} ist jetzt im Blick.` : `${res.added.length} Projekte sind jetzt im Blick.`);
    } catch (e) { disc.busy = null; rerenderDisc(); toast(errText(e)); }
  }
  async function setSetting(key, on) {
    try {
      settings = await call(key === 'autostart' ? 'set_autostart' : 'set_agent_access', { on });
      renderPanel(true);
    } catch (e) { toast('Nicht geändert: ' + errText(e)); }
  }
  async function removeProject(id) {
    const p = byId(id);
    try {
      await call('remove_project', { id });
      confirmRemove = null;
      await load(false);
      toast(`${p ? p.name : id} ist nicht mehr im Blick.`);
    } catch (e) { toast(errText(e)); }
  }

  document.addEventListener('click', async e => {
    const t = e.target;
    const sw = t.closest('.switch button');
    if (sw) { setPref('style', sw.dataset.style); window.scrollTo(0, 0); return; }
    const fbBtn = t.closest('.fb button');
    if (fbBtn) { e.stopPropagation(); setFeedback(fbBtn.parentElement, fbBtn.dataset.v); return; }
    const hide = t.closest('[data-hide]');
    if (hide) { const id = hide.dataset.hide; setPref('hidden_modules', [...hiddenMods(), id]); toast(`„${MOD_TITLES[id]}“ ausgeblendet.`, () => setPref('hidden_modules', [...hiddenMods()].filter(x => x !== id))); return; }
    const unhide = t.closest('[data-unhide]');
    if (unhide) { setPref('hidden_modules', [...hiddenMods()].filter(x => x !== unhide.dataset.unhide)); return; }
    if (t.closest('[data-toggle-rated]')) { showRated = !showRated; render(); return; }
    const st = t.closest('[data-status]');
    if (st && openId) { setStatus(st.dataset.status, $('#status-note').value); return; }
    if (t.closest('[data-open-settings]')) { openPanel(); return; }
    if (t.closest('[data-close-panel]')) { closePanel(); return; }
    const pk = t.closest('[data-pick]');
    if (pk) { pick(pk.dataset.pick, pk.dataset.target); return; }
    if (t.closest('[data-disc-all]')) { disc.checked = disc.checked.size === disc.candidates.length ? new Set() : new Set(disc.candidates.map(c => c.path)); rerenderDisc(); return; }
    if (t.closest('[data-disc-add]')) { addPaths([...disc.checked], disc.target); return; }
    if (t.closest('[data-disc-cancel]')) { disc = null; render(); return; }
    const setting = t.closest('[data-setting]');
    if (setting) { setSetting(setting.dataset.setting, setting.getAttribute('aria-checked') !== 'true'); return; }
    const copy = t.closest('[data-copy]');
    if (copy) { navigator.clipboard.writeText(copy.dataset.copy).then(() => toast('Kopiert.'), () => toast('Kopieren hat nicht geklappt.')); return; }
    const ask = t.closest('[data-remove-ask]');
    if (ask) { confirmRemove = ask.dataset.removeAsk; renderPanel(true); return; }
    if (t.closest('[data-remove-cancel]')) { confirmRemove = null; renderPanel(true); return; }
    const rm = t.closest('[data-remove]');
    if (rm) { removeProject(rm.dataset.remove); return; }
    const ext = t.closest('[data-external]');
    if (ext) { call('open_external', { target: ext.dataset.external }).catch(err => toast(errText(err))); return; }
    const op = t.closest('[data-open]');
    if (op) { openDrawer(op.dataset.open); return; }
    if (t.closest('[data-close]')) { closeDrawer(); }
  });
  document.addEventListener('change', async e => {
    if (e.target.matches('[data-disc]')) {
      const path = e.target.dataset.disc;
      if (e.target.checked) disc.checked.add(path); else disc.checked.delete(path);
      rerenderDisc();
      return;
    }
    if (e.target.id === 'status-note' && openId) {
      const p = byId(openId);
      try { await call('set_status', { id: openId, status: p.status, note: e.target.value }); p.status_note = e.target.value; toast('Notiz gespeichert.'); }
      catch (err) { toast('Notiz nicht gespeichert.'); }
    }
  });
  document.addEventListener('keydown', e => { if (e.key === 'Escape') { if (panelOpen) closePanel(); else closeDrawer(); } });
  $('#refresh').addEventListener('click', refreshFacts);

  // Reload when anything wrote to the database, such as an agent saving a card.
  const typing = () => { const a = document.activeElement; return !!a && ((a.tagName === 'INPUT' && a.type !== 'checkbox') || a.tagName === 'TEXTAREA'); };
  async function watch() {
    let v;
    try { v = await call('data_version'); } catch (e) { return; }
    if (typing()) return;
    const changed = lastVersion !== null && v !== lastVersion;
    lastVersion = v;
    if (changed && S && !refreshing) await load(false);
  }
  setInterval(watch, 2500);
  setInterval(refreshFacts, 20 * 60 * 1000);
  window.addEventListener('focus', () => { if (S) load(true); });

  load(true).then(refreshFacts);
})();
