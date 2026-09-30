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

  let S = null;           // server state
  let showRated = false;  // show suggestions the owner already rated away
  let openId = null;

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
    return `<div class="note" style="margin-top:24px">${missing.length === 1 ? 'Ein Projekt hat' : `${missing.length} Projekte haben`} noch keine Einschätzung: ${missing.map(p => plink(p.id)).join(', ')}. Der nächste Agent-Lauf schreibt sie (<code>lookout due</code> zeigt, was ansteht).</div>`;
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
      <dt>CI</dt><dd>${esc((CI[s.ci] || CI.unknown)[0])}${f.ci && f.ci.title ? ` · ${esc(f.ci.workflow || '')}: ${esc(f.ci.title)}` : ''}${f.ci && f.ci.url ? ` · <a href="${esc(f.ci.url)}" target="_blank" rel="noopener">Lauf öffnen</a>` : ''}</dd>
      </dl>` : `<p class="muted">${f.error === 'path_missing' ? 'Der Ordner existiert nicht mehr.' : 'Kein Git-Repository, daher keine Git-Fakten.'}</p>`;
    let body = `<button class="close" data-close type="button">Schließen</button>
      <h2 class="pname">${dot(p.id)}${esc(p.name)} ${phasePill(p)}${isNew(p.card_updated_at) ? '<span class="pill new">neu</span>' : ''}</h2>
      ${c ? `<div class="one">${esc(c.one_liner)}</div>` : ''}
      <div class="stack">${c && c.stack ? `${esc(c.stack)} · ` : ''}${esc(p.path)}</div>
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

  function renderEmpty() {
    return `<div class="empty"><h2>Noch keine Projekte im Blick</h2>
      <p class="muted">Registriere deine Projekte im Terminal. Lookout misst dann sofort die Git-Fakten, und ein Agent schreibt beim nächsten Lauf die Einschätzungen.</p>
      <p><code>lookout projects discover C:\\Development\\projects</code> zeigt Kandidaten, <code>lookout projects add &lt;pfad&gt;</code> registriert einen.</p></div>`;
  }

  function render() {
    const d = new Date(S.today + 'T12:00:00');
    $('#date').textContent = d.toLocaleDateString('de-DE', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
    const collected = S.projects.map(p => p.facts_collected_at).filter(Boolean).sort()[0];
    $('#fresh').textContent = collected ? `Fakten von vor ${minutesAgo(collected)} Min` : '';
    document.querySelectorAll('.switch button').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.style === S.prefs.style)));
    $('#view').innerHTML = !S.projects.length ? renderEmpty() : S.prefs.style === 'board' ? renderBoard() : renderHandover();
    renderFoot();
    if (openId) openDrawer(openId, true);
  }

  // ---------- server calls ----------
  async function api(path, body) {
    const res = await fetch(path, body === undefined ? {} : { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    if (!res.ok) throw new Error(`${res.status}`);
    return res.json();
  }
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
    try { S = await api(`/api/state${visit ? '?visit=true' : ''}`); render(); }
    catch (e) { $('#view').innerHTML = '<p class="note" style="margin-top:30px">Die Lookout-App antwortet nicht. Läuft <code>lookout serve</code>?</p>'; }
  }
  async function setFeedback(el, v) {
    const id = el.dataset.item, prev = S.feedback[id] ? S.feedback[id].value : null;
    const value = prev === v ? null : v;
    const send = val => api('/api/feedback', { item_id: id, value: val, project_id: el.dataset.project || null, title: el.dataset.title });
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
  async function setPref(key, value) { S.prefs[key] = value; render(); try { await api('/api/prefs', { key, value }); } catch (e) { toast('Einstellung nicht gespeichert.'); } }

  document.addEventListener('click', async e => {
    const sw = e.target.closest('.switch button');
    if (sw) { setPref('style', sw.dataset.style); window.scrollTo(0, 0); return; }
    const fbBtn = e.target.closest('.fb button');
    if (fbBtn) { e.stopPropagation(); setFeedback(fbBtn.parentElement, fbBtn.dataset.v); return; }
    const hide = e.target.closest('[data-hide]');
    if (hide) { const id = hide.dataset.hide; setPref('hidden_modules', [...hiddenMods(), id]); toast(`„${MOD_TITLES[id]}“ ausgeblendet.`, () => setPref('hidden_modules', [...hiddenMods()].filter(x => x !== id))); return; }
    const unhide = e.target.closest('[data-unhide]');
    if (unhide) { setPref('hidden_modules', [...hiddenMods()].filter(x => x !== unhide.dataset.unhide)); return; }
    if (e.target.closest('[data-toggle-rated]')) { showRated = !showRated; render(); return; }
    const st = e.target.closest('[data-status]');
    if (st && openId) {
      try { await api(`/api/projects/${encodeURIComponent(openId)}/status`, { status: st.dataset.status, note: $('#status-note').value }); await load(false); }
      catch (err) { toast('Status nicht gespeichert.'); }
      return;
    }
    const op = e.target.closest('[data-open]');
    if (op) { openDrawer(op.dataset.open); return; }
    if (e.target.closest('[data-close]')) { closeDrawer(); }
  });
  document.addEventListener('change', async e => {
    if (e.target.id === 'status-note' && openId) {
      const p = byId(openId);
      try { await api(`/api/projects/${encodeURIComponent(openId)}/status`, { status: p.status, note: e.target.value }); p.status_note = e.target.value; toast('Notiz gespeichert.'); }
      catch (err) { toast('Notiz nicht gespeichert.'); }
    }
  });
  document.addEventListener('keydown', e => { if (e.key === 'Escape') closeDrawer(); });
  $('#refresh').addEventListener('click', async () => {
    const b = $('#refresh');
    b.disabled = true; b.textContent = 'Misst …';
    try { S = await api('/api/refresh', {}); render(); } catch (err) { toast('Aktualisieren hat nicht geklappt.'); }
    b.disabled = false; b.textContent = 'Aktualisieren';
  });
  document.addEventListener('visibilitychange', () => { if (!document.hidden && S) load(false); });

  load(true);
})();
