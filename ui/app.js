(function () {
  'use strict';

  const PALETTE = ['#e8870a', '#4fae7a', '#5b8def', '#c0674a', '#9a7bd0', '#d06a9c', '#3aa6b0', '#b3a23a', '#7f9c4a', '#c9853f'];
  const PHASE = { active: 'aktiv', cooling: 'kühlt ab', dormant: 'schläft', done: 'fertig', maintenance: 'Wartung' };
  const STATUS = { active: 'aktiv', parked: 'geparkt', done: 'fertig', archived: 'archiviert' };
  const KIND = { worktree: 'Worktree', branch: 'Branch', uncommitted: 'nicht committet', unpushed: 'nicht gepusht', ci: 'CI', flagged: 'FLAGGED', todo: 'TODO', plan: 'Plan', other: 'Sonstiges' };
  const TYPE = { next: 'Nächster Schritt', feature: 'Feature', cleanup: 'Aufräumen', park: 'Parken' };
  const CI = { green: ['grün', 'good'], red: ['rot', 'bad'], running: ['läuft', ''], none: ['keins', ''], unknown: ['unbekannt', ''], skipped: ['—', ''] };
  const VERDICT = { park: ['parken', 'warn'], revive: ['zurückholen', 'good'], finish: ['fertig machen', 'good'], archive: ['archivieren', 'bad'] };
  const MOD_TITLES = {
    focus: 'Heute zuerst', projects: 'Deine Projekte', waiting: 'Wartet auf dich', safety: 'Sicherheitslage', recent: 'Was passiert ist',
    across: 'Quer durch alle Projekte', ideas: 'Ideen', pulse: 'Puls', kpis: 'Lage auf einen Blick', cards: 'Projekte',
  };

  let S = null;           // state from the app core
  let showRated = false;  // show suggestions the owner already rated away
  let showAllRecent = false;
  let noteFor = null;     // item whose "why?" field shows right after a later/down rating
  let answering = null;   // question whose answer field is open
  const drafts = {};      // unsent note and answer text per item, kept across re-renders
  let openId = null;      // project whose drawer is open
  let hbOpen = false;     // the drawer's handbook section is expanded
  let since = null;       // { id, data, dismissed }: what happened since the owner last opened that project
  let panelOpen = false;  // settings panel
  let settings = null;    // autostart, agent access, paths
  let disc = null;        // project search: { target, result (from discover), checked:Set, busy }
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
  // the owner's reply to a question: answered (with the answer as note) or done
  const reply = id => { const f = id && S.feedback[id]; return f && (f.value === 'answered' || f.value === 'done') ? f : null; };
  const openQs = c => c.waiting_on_you.filter(w => !reply(w.id));

  // ---------- facts-derived safety ----------
  function safety(p) {
    const f = p.facts || {};
    const d = f.dirty || {};
    const out = { remote: f.has_remote, unsaved: null, level: null, ahead: f.ahead || 0, ci: (f.ci || {}).state || 'unknown', leftovers: null };
    if (d.files) {
      const lines = (d.lines_added || 0) + (d.lines_removed || 0);
      const age = d.oldest_change ? daysSince(d.oldest_change) : 0;
      out.unsaved = `${num(d.files)} Dateien, +${num(d.lines_added)}/−${num(d.lines_removed)}${age > 0 ? `, seit ${age} ${age === 1 ? 'Tag' : 'Tagen'}` : ''}`;
      out.unsavedShort = `${num(d.files)} ${d.files === 1 ? 'Datei' : 'Dateien'} offen${age > 0 ? ` · ${age} T` : ''}`;
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

  function fbButtons(id) {
    if (!id) return '';
    const cur = rated(id);
    return `<span class="fb" data-item="${esc(id)}">${[['up', 'gut'], ['later', 'später'], ['down', 'Quatsch']].map(([v, l]) => `<button type="button" data-v="${v}" aria-pressed="${cur === v}">${l}</button>`).join('')}</span>`;
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

  // right after a later/down rating the card turns into an optional "why?" field
  function noteForm(id, label) {
    const down = S.feedback[id] && S.feedback[id].value === 'down';
    return `<div class="idea noteform"><div class="kind">${down ? 'Abgelehnt' : 'Später'} · ${esc(label)}</div>
      <div class="why">Magst du kurz sagen, warum? Die Agents richten künftige Vorschläge danach.</div>
      <div class="inrow"><input type="text" aria-label="Warum" data-draft="${esc(id)}" data-kind="note" maxlength="300" value="${esc(drafts[id] || '')}" placeholder="${down ? 'z. B. passt nicht zum Projekt' : 'z. B. erst nach dem Release'}">
      <button type="button" class="btn primary" data-note-save="${esc(id)}">Merken</button><button type="button" class="btn" data-note-skip>Überspringen</button></div></div>`;
  }
  const myNote = id => { const f = S.feedback[id]; return f && f.note && !reply(id) ? `<div class="mynote">Deine Notiz: ${esc(f.note)}</div>` : ''; };

  function ideaCard(s, projects, projectId, kind) {
    if (noteFor === s.id) return noteForm(s.id, s.title);
    if (hiddenByRating(s.id)) return '';
    return `<div class="idea">${kind ? `<div class="kind">${kind}</div>` : ''}<div class="h"><div class="t">${s.severity ? `<span class="sev-${esc(s.severity)}">●</span> ` : ''}${esc(s.title)}</div>${s.effort ? `<span class="effort">${esc(s.effort)}</span>` : ''}</div>
      <div class="why">${esc(s.why || s.pitch)}</div>
      ${s.because ? `<div class="why"><b>Weil:</b> ${esc(s.because)}</div>` : ''}
      ${s.evidence ? `<details><summary class="more">Beleg</summary><div class="ev">${esc(s.evidence)}</div></details>` : ''}
      ${myNote(s.id)}
      <div class="foot"><div class="who">${(projects || []).map(plink).join('')}${s.type ? `<span class="tag">${esc(TYPE[s.type] || s.type)}</span>` : ''}</div>${fbButtons(s.id)}</div></div>`;
  }

  function blockFocus() {
    const t = (S.portfolio && S.portfolio.focus) || [];
    if (!t.length) return '';
    return mod('focus', 'Heute zuerst', 'Über alle Projekte hinweg ausgewählt', `<div class="focus">${t.slice(0, 3).map((it, i) => `
      <div class="item${reply(it.id) ? ' done' : ''}"><div class="n">${i + 1}</div><div><div class="t">${esc(it.title)}</div>
      <div class="meta">${plink(it.project)}<span class="effort">${esc(it.effort)}</span>${it.id ? `<button type="button" class="check" data-focus-done="${esc(it.id)}" aria-pressed="${!!reply(it.id)}">${reply(it.id) ? '✓ erledigt' : 'erledigt?'}</button>` : ''}</div>
      <div class="why">${esc(it.why)}</div></div></div>`).join('')}</div>`);
  }

  // what the facts say about a project, as a few short badges
  function signals(p) {
    const f = p.facts || {};
    if (!f.is_git) return `<span class="tag">${f.error === 'path_missing' ? 'Ordner fehlt' : 'kein Git'}</span>`;
    const s = safety(p);
    const out = [];
    if (!s.remote) out.push('<span class="tag bad">kein Remote</span>');
    if (s.unsavedShort) out.push(`<span class="tag ${s.level}">${esc(s.unsavedShort)}</span>`);
    if (s.ahead) out.push(`<span class="tag warn">${s.ahead} ungepusht</span>`);
    if (s.ci === 'red') out.push('<span class="tag bad">CI rot</span>');
    if (!out.length) out.push('<span class="tag good">gesichert</span>');
    return out.join('');
  }
  const risk = p => { const s = safety(p); return (p.facts && p.facts.is_git && !s.remote ? 2 : 0) + (s.level === 'bad' ? 2 : s.level === 'warn' ? 1 : 0) + (s.ci === 'red' ? 1 : 0) + (s.ahead ? 1 : 0); };

  function blockProjects() {
    const list = [...visible()].sort((a, b) => risk(b) - risk(a) || (lastWork(b) || '').localeCompare(lastWork(a) || ''));
    if (!list.length) return '';
    const tile = p => {
      const c = p.card, waiting = c ? openQs(c).length : 0;
      const fresh = isNew(p.card_updated_at) ? '<span class="pill new">neu</span>' : '';
      return `<button class="tile" type="button" data-open="${esc(p.id)}">
        <div class="h"><span class="name">${dot(p.id)}${esc(p.name)}</span><span class="pills">${fresh}${p.status !== 'active' ? `<span class="pill unknown">${esc(STATUS[p.status])}</span>` : phasePill(p)}</span></div>
        <p class="voice">${c ? `„${esc(c.personality || c.headline)}“` : '<span class="faint">Noch keine Einschätzung. Kommt beim nächsten Agent-Lauf.</span>'}</p>
        <div class="badges">${signals(p)}${waiting ? `<span class="tag ask">${waiting} ${waiting === 1 ? 'Entscheidung' : 'Entscheidungen'}</span>` : ''}</div>
        <div class="tfoot">${spark(weekly(p), color(p.id), 22)}<span>zuletzt ${ago(lastWork(p))}</span></div>
      </button>`;
    };
    return mod('projects', 'Deine Projekte', 'Die riskantesten zuerst. Klick öffnet den Steckbrief', `<div class="tiles">${list.map(tile).join('')}</div>`);
  }

  // one question: answer it or mark it done; answered ones show the reply until the next card
  function question(w, withSource) {
    const f = reply(w.id);
    if (f) return `<li class="q replied"><span class="qt">${esc(w.title)}</span><div class="qr">${f.value === 'answered' ? `Deine Antwort: ${esc(f.note)}` : 'Erledigt'} · <button type="button" class="linkish" data-reply-undo="${esc(w.id)}">zurücknehmen</button></div></li>`;
    const acts = w.id && answering !== w.id ? `<span class="qa"><button type="button" data-answer="${esc(w.id)}">antworten</button><button type="button" data-qdone="${esc(w.id)}">erledigt</button></span>` : '';
    const form = answering === w.id ? `<div class="inrow"><input type="text" aria-label="Deine Antwort" data-draft="${esc(w.id)}" data-kind="answer" maxlength="600" value="${esc(drafts[w.id] || '')}" placeholder="Deine Antwort, der nächste Lauf nimmt sie in die Karte auf">
      <button type="button" class="btn primary" data-answer-save="${esc(w.id)}">Senden</button><button type="button" class="btn" data-answer-cancel>Abbrechen</button></div>` : '';
    const src = w.source ? (withSource ? `<div class="qsrc mono">${esc(w.source)}</div>` : '') : '';
    return `<li class="q"${w.source && !withSource ? ` title="Quelle: ${esc(w.source)}"` : ''}><div class="qrow"><span class="qt">${esc(w.title)}</span>${acts}</div>${src}${form}</li>`;
  }

  function blockWaiting(limitPer = 2) {
    const groups = visible().filter(p => p.card && openQs(p.card).length);
    if (!groups.length) return '';
    const total = groups.reduce((a, p) => a + openQs(p.card).length, 0);
    const body = groups.map(p => {
      const items = openQs(p.card);
      // an open answer field stays visible even when its question sits below the fold
      const cut = Math.max(limitPer, items.findIndex(w => w.id === answering) + 1);
      const head = items.slice(0, cut), rest = items.slice(cut);
      return `<div class="group"><div class="group-h">${plink(p.id)}<span class="count">${items.length}</span></div>
        <ul class="qs">${head.map(w => question(w)).join('')}</ul>${rest.length ? `<details><summary class="more">+ ${rest.length} weitere</summary><ul class="qs">${rest.map(w => question(w)).join('')}</ul></details>` : ''}</div>`;
    }).join('');
    return mod('waiting', 'Wartet auf dich', `${total} ${total === 1 ? 'Entscheidung' : 'Entscheidungen'}, die nur du treffen kannst. Deine Antworten lesen die Agents beim nächsten Lauf`, `<div class="groups">${body}</div>`);
  }

  function blockRecent(limit = 10) {
    const items = visible().flatMap(p => (p.card ? p.card.recent_changes : []).map(c => ({ ...c, project: p.id }))).filter(c => daysSince(c.date) <= 14).sort((a, b) => b.date.localeCompare(a.date));
    if (!items.length) return '';
    const prevDay = S.previous_visit ? S.previous_visit.slice(0, 10) : null;
    const shownItems = showAllRecent ? items : items.slice(0, limit);
    let last = '';
    const body = shownItems.map(c => {
      const h = c.date !== last ? `<div class="day-h">${fmtShort(c.date)} · ${ago(c.date)}</div>` : '';
      last = c.date;
      const fresh = prevDay && c.date >= prevDay ? ' <span class="pill new">neu</span>' : '';
      const p = byId(c.project);
      const head = `<span class="t">${esc(c.title)}${fresh}</span><span class="pmark">${dot(c.project)}${esc(p ? p.name : c.project)}</span>`;
      return `${h}${c.detail ? `<details class="ev-item"><summary>${head}</summary><p>${esc(c.detail)}</p></details>` : `<div class="ev-item"><div class="sum">${head}</div></div>`}`;
    }).join('');
    const more = items.length > limit ? `<button class="more" type="button" data-recent-all>${showAllRecent ? 'weniger anzeigen' : `alle ${items.length} anzeigen`}</button>` : '';
    return mod('recent', 'Was passiert ist', 'Die größeren Änderungen der letzten 14 Tage. Klick zeigt Details', `<div class="events">${body}</div>${more}`);
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

  function blockPulse() {
    const list = visible().filter(p => p.facts && p.facts.is_git);
    if (!list.length) return '';
    const shares = list.map(p => [p.id, p.facts.commits.last_30d]).sort((a, b) => b[1] - a[1]);
    const total = shares.reduce((a, [, v]) => a + v, 0) || 1;
    const bar = shares.map(([id, v]) => v ? `<span style="width:${(v / total * 100).toFixed(2)}%;background:${color(id)}" title="${esc(byId(id).name)}: ${v}"></span>` : '').join('');
    const legend = shares.map(([id, v]) => `<span>${dot(id)}${esc(byId(id).name)} <b class="mono">${num(v)}</b></span>`).join('');
    const note = (S.portfolio && S.portfolio.attention_note) || 'Wohin deine Commits der letzten 30 Tage gingen';
    return mod('pulse', 'Puls', note, `<div class="attn-bar" role="img" aria-label="Anteil der Commits in 30 Tagen">${bar}</div><div class="attn-legend">${legend}</div>`);
  }

  // patterns, synergies and verdicts: what shows only when all projects lie side by side
  function blockAcross() {
    const pf = S.portfolio || {};
    const verdict = r => {
      const v = VERDICT[r.verdict] || [r.verdict, ''];
      const name = byId(r.project) ? byId(r.project).name : r.project;
      if (noteFor === r.id) return noteForm(r.id, `${v[0]}: ${name}`);
      if (hiddenByRating(r.id)) return '';
      return `<div class="idea"><div class="kind">Parken oder zurückholen?</div><div class="h"><div class="t">${esc(name)}</div><span class="tag ${v[1]}">${esc(v[0])}</span></div>
        <div class="why">${esc(r.why)}</div>${myNote(r.id)}<div class="foot"><div class="who">${plink(r.project)}</div>${fbButtons(r.id)}</div></div>`;
    };
    const cards = [
      ...(pf.patterns || []).map(s => ideaCard(s, s.projects, null, 'Muster')),
      ...(pf.synergies || []).map(s => ideaCard(s, s.projects, null, 'Querverbindung')),
      ...(pf.park_or_revive || []).map(verdict),
    ].filter(Boolean);
    return cards.length ? mod('across', 'Quer durch alle Projekte', 'Was man erst sieht, wenn alle nebeneinander liegen', `<div class="cards3">${cards.join('')}</div>`) : '';
  }

  function blockIdeas() {
    const fromProjects = visible().map(p => {
      const list = ((p.card || {}).suggestions || []).filter(s => !hiddenByRating(s.id));
      const s = list.find(x => x.type === 'feature') || list[0];
      return s ? ideaCard(s, [p.id], p.id, 'Aus dem Projekt') : '';
    });
    const apps = ((S.portfolio || {}).new_apps || []).map(s => ideaCard(s, [], null, 'Neue App'));
    const cards = [...fromProjects, ...apps].filter(Boolean);
    return cards.length ? mod('ideas', 'Ideen', 'Pro Projekt der interessanteste Vorschlag, dazu neue App-Ideen. Weitere stehen im Steckbrief', `<div class="cards3">${cards.join('')}</div>`) : '';
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
    const aside = blockRecent() + blockPulse();
    return `<div class="handover">
      <div class="greet"><div class="kicker">Übergabe · ${n} ${n === 1 ? 'Projekt' : 'Projekte'} ${fresh}</div><h2>${esc(greeting)}</h2></div>
      ${missingCardsNote()}
      ${blockFocus()}${blockProjects()}
      <div class="duo">${blockWaiting()}${aside ? `<div class="aside">${aside}</div>` : ''}</div>
      ${blockAcross()}${blockIdeas()}
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
      ${c ? `<div class="next"><span class="faint" style="font-size:11.5px;letter-spacing:.06em;text-transform:uppercase">Wiedereinstieg</span><b>${esc(c.reentry.next_step)}</b><span><span class="effort">${esc(c.reentry.effort)}</span> · ${openQs(c).length} Entscheidungen · ${c.open_ends.length} offene Enden</span></div>` : ''}
    </button>`;
  }

  function renderBoard() {
    const list = visible();
    const git = list.filter(p => p.facts && p.facts.is_git);
    const noRemote = git.filter(p => !p.facts.has_remote).length;
    const unsaved = git.filter(p => (p.facts.dirty || {}).files || p.facts.ahead).length;
    const red = git.filter(p => (p.facts.ci || {}).state === 'red').length;
    const waiting = list.reduce((a, p) => a + (p.card ? openQs(p.card).length : 0), 0);
    const kpis = `<div class="kpis">
      <div class="kpi ${noRemote ? 'bad' : 'ok'}"><div class="v">${noRemote}</div><div class="l">Repos ohne Backup</div></div>
      <div class="kpi ${unsaved ? 'warn' : 'ok'}"><div class="v">${unsaved}</div><div class="l">mit ungesicherter Arbeit</div></div>
      <div class="kpi ${red ? 'bad' : 'ok'}"><div class="v">${red}</div><div class="l">mit rotem CI</div></div>
      <div class="kpi"><div class="v">${waiting}</div><div class="l">Entscheidungen offen</div></div></div>`;
    const grid = mod('cards', 'Projekte', 'Sortiert nach letzter Arbeit. Klick öffnet den Steckbrief', `<div class="grid">${byLastWork(list).map(card).join('')}</div>`);
    return `${mod('kpis', 'Lage auf einen Blick', null, kpis)}
      <div class="board"><div>${grid}${blockSafety()}${blockRecent()}${blockPulse()}</div>
      <aside class="side">${blockFocus()}${blockWaiting(1)}${blockAcross()}</aside></div>`;
  }

  // ---------- handbook ----------
  // how to start a project, how it is built, where things are: for coming back after a long time
  const HB_LISTS = ['run', 'places', 'read_first', 'needs', 'terms'];
  const hbEmpty = h => !h || (!h.architecture && !HB_LISTS.some(k => (h[k] || []).length));
  function hbParts(h) {
    h = h || {};
    const kv = (items, key, val, keyClass) => items && items.length ? `<dl class="kv">${items.map(x => `<dt class="${keyClass}">${esc(x[key])}</dt><dd>${esc(x[val])}</dd>`).join('')}</dl>` : '';
    return {
      run: (h.run || []).length ? `<div class="hb-runs">${h.run.map(r => `<div class="hb-run"><span class="w">${esc(r.what)}</span><code class="mono">${esc(r.how)}</code><button class="linkish" type="button" data-copy="${esc(r.how)}" aria-label="${esc(r.what)} kopieren">kopieren</button></div>`).join('')}</div>` : '',
      arch: h.architecture ? `<p class="hb-arch">${esc(h.architecture)}</p>` : '',
      places: kv(h.places, 'path', 'what', 'mono'),
      read: kv(h.read_first, 'path', 'why', 'mono'),
      needs: (h.needs || []).length ? `<ul class="bullets">${h.needs.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : '',
      terms: kv(h.terms, 'term', 'meaning', 'term'),
    };
  }
  const hbSec = (title, body) => body ? `<div class="hb-sec"><h3>${esc(title)}</h3>${body}</div>` : '';
  function handbookBody(h) {
    const x = hbParts(h);
    return hbSec('So startest du es', x.run) + hbSec('Wie es gebaut ist', x.arch) + hbSec('Wo was liegt', x.places)
      + hbSec('Zuerst lesen', x.read) + hbSec('Braucht', x.needs) + hbSec('Begriffe', x.terms);
  }

  function renderHandbook() {
    const list = [...visible()].sort((a, b) => a.name.localeCompare(b.name, 'de'));
    const sheet = p => {
      const c = p.card, x = hbParts(c && c.handbook);
      const missing = c && hbEmpty(c.handbook) ? '<div class="faint hb-sec" style="font-size:13px">Noch kein Handbuch. Der nächste Agent-Lauf schreibt es.</div>' : '';
      return `<article class="sheet">
        <div class="h">${plink(p.id)}<span class="pills">${p.status !== 'active' ? `<span class="pill unknown">${esc(STATUS[p.status])}</span>` : phasePill(p)}</span></div>
        ${c ? `<div class="one">${esc(c.one_liner)}</div>${c.stack ? `<div class="stackline mono">${esc(c.stack)}</div>` : ''}` : '<div class="faint one">Noch keine Karte.</div>'}
        ${missing}${hbSec('So startest du es', x.run)}${hbSec('Zuerst lesen', x.read)}
        ${c ? `<div class="state"><span class="faint">Stand:</span> ${esc(c.headline)} <span class="faint">· zuletzt ${ago(lastWork(p))}</span></div>` : ''}
        <div class="path"><button class="linkish mono" type="button" data-external="${esc(p.path)}" title="Im Explorer öffnen">${esc(p.path)}</button></div>
      </article>`;
    };
    return `<div class="handbook-view">${dmod('Handbuch', `<div class="sheets">${list.map(sheet).join('')}</div>`, 'Alle Projekte zum Nachschlagen: wofür, womit, wie starten. Der Name öffnet den Steckbrief mit dem ganzen Handbuch')}</div>`;
  }

  // what happened since the owner last opened this project, when that was a while ago
  function sinceHtml(p) {
    if (!since || since.id !== p.id || !since.data || since.dismissed) return '';
    const s = since.data;
    const when = s.days < 28 ? `vor ${s.days} Tagen` : `vor ${Math.round(s.days / 7)} Wochen`;
    const counts = [
      s.commits == null ? '' : s.commits ? `${num(s.commits)} ${s.commits === 1 ? 'Commit' : 'Commits'}` : 'keine Commits',
      s.card_updates ? `Karte ${s.card_updates}× aktualisiert` : 'Karte unverändert',
    ].filter(Boolean).join(' · ');
    const lines = [];
    if (s.phase) lines.push(`<li>Phase: ${esc(PHASE[s.phase.from] || s.phase.from)} → ${esc(PHASE[s.phase.to] || s.phase.to || '—')}</li>`);
    s.milestones.forEach(m => lines.push(`<li><b>Meilenstein:</b> ${esc(m.title)} <span class="faint mono">${fmtShort(m.date)}</span></li>`));
    s.recent_changes.forEach(ch => lines.push(`<li>${esc(ch.title)} <span class="faint mono">${fmtShort(ch.date)}</span></li>`));
    const body = !s.commits && !s.card_updates ? '<div class="muted">Seitdem ist nichts passiert.</div>' : lines.length ? `<ul class="bullets">${lines.join('')}</ul>` : '';
    return `<section class="since"><div class="since-h"><b>Seit du zuletzt reingeschaut hast</b><span class="faint">${when} · ${counts}</span><button class="linkish" type="button" data-since-close>ausblenden</button></div>${body}</section>`;
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
      ${sinceHtml(p)}
      ${c && c.personality ? `<div class="say">„${esc(c.personality)}“</div>` : ''}
      <div style="margin-top:14px">${spark(weekly(p), color(p.id), 48)}${c && c.phase_evidence ? `<div class="faint" style="font-size:12.5px">${esc(c.phase_evidence)}</div>` : ''}</div>`;
    if (!c) {
      body += `<div class="note" style="margin-top:20px">Noch keine Einschätzung. Der nächste Agent-Lauf schreibt sie. Bis dahin siehst du nur die Git-Fakten.</div>`;
    } else {
      const v = c.vision || {};
      const suggestions = c.suggestions.map(sg => ideaCard(sg, [], p.id)).filter(Boolean).join('');
      body += dmod('Wiedereinstieg', `<div class="reentry"><div class="t">${esc(c.reentry.next_step)}</div><div><span class="effort">${esc(c.reentry.effort)}</span></div>${c.reentry.why ? `<div class="muted" style="font-size:14px">${esc(c.reentry.why)}</div>` : ''}</div>`)
        + dmod('Wartet auf dich', c.waiting_on_you.length ? `<ul class="qs">${c.waiting_on_you.map(w => question(w, true)).join('')}</ul>` : '')
        + dmod('Offene Enden', c.open_ends.length ? `<div class="list">${c.open_ends.map(o => `<div class="row"><div class="t">${esc(o.title)}</div><div style="display:flex;gap:6px;align-items:start"><span class="tag ${o.kind === 'uncommitted' || o.kind === 'ci' ? 'bad' : o.kind === 'unpushed' ? 'warn' : ''}">${esc(KIND[o.kind] || o.kind)}</span>${o.age_days != null ? `<span class="faint mono" style="font-size:12px">${o.age_days} T</span>` : ''}</div>${o.detail ? `<div class="d">${esc(o.detail)}</div>` : ''}${o.source ? `<div class="d mono" style="font-size:12px">${esc(o.source)}</div>` : ''}</div>`).join('')}</div>` : '', 'Angefangenes, Liegengebliebenes, Ungesichertes')
        + dmod('Zuletzt passiert', c.recent_changes.length ? `<div class="list">${c.recent_changes.map(ch => `<div class="row"><div class="t">${esc(ch.title)}</div><div class="faint mono" style="font-size:12px">${fmtShort(ch.date)}</div>${ch.detail ? `<div class="d">${esc(ch.detail)}</div>` : ''}</div>`).join('')}</div>` : '')
        + dmod('Vorschläge', suggestions ? `<div class="cards2">${suggestions}</div>` : '')
        + dmod('Vision und Fortschritt', v.statement || v.progress_note ? `<div>${v.statement ? `<div>${esc(v.statement)}</div>` : ''}${v.progress_pct != null ? `<div class="prog"><span style="width:${Number(v.progress_pct)}%"></span></div><div class="faint mono" style="font-size:12px">${Number(v.progress_pct)} %</div>` : ''}${v.progress_note ? `<div class="muted" style="font-size:14px;margin-top:6px">${esc(v.progress_note)}</div>` : ''}</div>` : '')
        + dmod('Was es heute kann', c.features.length ? `<div class="feat">${c.features.map(x => `<span>${esc(x)}</span>`).join('')}</div>` : '')
        + dmod('Meilensteine', c.milestones.length ? `<ol class="timeline">${c.milestones.map(m => `<li><span class="when">${fmtDate(m.date)}</span><span class="rail"></span><span class="what">${esc(m.title)}</span></li>`).join('')}</ol>` : '')
        + dmod('Gesundheit', c.health_notes.length ? `<ul class="bullets">${c.health_notes.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : '')
        + dmod('Bewusst so entschieden', (c.decisions || []).length ? `<ul class="bullets">${c.decisions.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : '', 'Aus den Unterlagen des Projekts; die Agents werten das nicht als Risiko')
        + dmod('Querverbindungen', c.cross_project_hints.length ? `<ul class="bullets">${c.cross_project_hints.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : '')
        + (hbEmpty(c.handbook) ? '' : `<details class="mod hb" data-hb${hbOpen ? ' open' : ''}><summary class="mod-head"><div><h2>Handbuch <span class="chev" aria-hidden="true">▸</span></h2><div class="sub">Starten, Aufbau, wo was liegt: fürs Wiederreinkommen nach längerer Pause</div></div></summary>${handbookBody(c.handbook)}</details>`);
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
    if (!keepScroll) {
      since = null;
      call('open_project', { id }).then(r => { if (openId === id && r.since) { since = { id, data: r.since }; openDrawer(id, true); } }).catch(() => {});
    }
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
    if (ratedCount) parts.push(`${ratedCount} ${ratedCount === 1 ? 'bewerteter Vorschlag' : 'bewertete Vorschläge'} ${showRated ? (ratedCount === 1 ? 'wird angezeigt' : 'werden angezeigt') : 'ausgeblendet'} <button type="button" data-toggle-rated>${showRated ? 'wieder ausblenden' : 'anzeigen'}</button>`);
    if (archived.length) parts.push(`Archiviert: ${archived.map(p => plink(p.id)).join(', ')}`);
    parts.push(`Karten schreiben Agents mit <code class="mono">lookout</code> · Fakten misst die App selbst`);
    $('#foot').innerHTML = parts.map(x => `<span>${x}</span>`).join('');
  }

  // ---------- adding projects ----------
  // while a search runs or its checklist is open, the list replaces the add button
  const discBusyOrListing = target => disc && disc.target === target && (disc.busy || disc.result.candidates.length);
  function discHtml(target) {
    if (!disc || disc.target !== target) return '';
    if (disc.busy) return `<p class="muted disc-msg">${esc(disc.busy)}</p>`;
    const r = disc.result, folder = `<span class="mono">${esc(r.folder)}</span>`;
    if (!r.candidates.length) {
      const msg = r.already_watched
        ? (r.folder_is_repo ? `${folder} ist schon im Blick.` : `Alle Git-Repos in ${folder} sind schon im Blick.`)
        : `In ${folder} habe ich keine Git-Repos gefunden. Wähle den Ordner, in dem deine Projekte liegen, oder direkt ein Projekt.`;
      return `<p class="muted disc-msg">${msg}</p>`;
    }
    const n = disc.checked.size, all = n === r.candidates.length;
    const head = r.folder_is_repo
      ? (r.candidates.length > 1 ? `<b>${esc(r.candidates[0].name)}</b> ist ausgewählt. Daneben liegen noch diese Repos, falls du sie auch willst:` : `<b>${esc(r.candidates[0].name)}</b> ist ausgewählt.`)
      : `${r.candidates.length} Git-Repos gefunden. Angehakt sind die, an denen zuletzt gearbeitet wurde.`;
    const rows = r.candidates.map(c => `<label class="disc-row"><input type="checkbox" data-disc="${esc(c.path)}" ${disc.checked.has(c.path) ? 'checked' : ''}><span class="t">${esc(c.name)}</span><span class="faint">${c.last_commit ? `letzter Commit ${ago(c.last_commit)}` : 'noch kein Commit'}</span></label>`).join('');
    return `<div class="disc"><div class="disc-head"><span>${head}</span>${r.candidates.length > 1 ? `<button class="linkish" type="button" data-disc-all>${all ? 'keins' : 'alle'} auswählen</button>` : ''}</div>
      <div class="disc-list">${rows}</div>
      <div class="actions"><button class="btn primary" type="button" data-disc-add ${n ? '' : 'disabled'}>${n === 1 ? '1 Projekt' : `${n} Projekte`} hinzufügen</button><button class="btn" type="button" data-disc-cancel>Abbrechen</button></div></div>`;
  }

  function renderEmpty() {
    return `<div class="empty"><div class="kicker">Willkommen</div><h2>Welche Projekte soll Lookout im Blick behalten?</h2>
      <p class="muted">Wähle den Ordner, in dem deine Projekte liegen, oder direkt ein einzelnes Projekt. Lookout zeigt dir die gefundenen Git-Repos zum Abhaken und misst sofort, wie es um sie steht. Die Einschätzungen schreibt danach ein Agent; wie du den einrichtest, steht in den Einstellungen.</p>
      ${discBusyOrListing('main') ? '' : '<div class="actions"><button class="btn primary" type="button" data-pick data-target="main">Ordner wählen …</button></div>'}
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
        ${discBusyOrListing('panel') ? '' : '<div class="actions"><button class="btn" type="button" data-pick data-target="panel">Projekte hinzufügen …</button></div>'}
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
    const style = { board: renderBoard, handbook: renderHandbook }[S.prefs.style] || renderHandover;
    $('#view').innerHTML = !S.projects.length ? renderEmpty() : style();
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
  // Saves the owner's reaction to an item (a rating, an answer, "done"); `note` undefined keeps the stored one.
  async function react(id, value, note, msg) {
    const prev = S.feedback[id] ? { ...S.feedback[id] } : null;
    try {
      await call('set_feedback', { item_id: id, value, note: note === undefined ? null : note });
      if (value) S.feedback[id] = { value, at: new Date().toISOString(), note: note === undefined ? (prev ? prev.note : '') : note }; else delete S.feedback[id];
      render();
      if (msg) toast(msg, async () => {
        noteFor = null;
        try {
          await call('set_feedback', { item_id: id, value: prev ? prev.value : null, note: prev ? prev.note : null });
          if (prev) S.feedback[id] = prev; else delete S.feedback[id];
        } catch (e) { toast('Rückgängig hat nicht geklappt: ' + errText(e)); }
        render();
      });
      return true;
    } catch (e) { toast('Nicht gespeichert: ' + errText(e)); return false; }
  }
  // the drawer renders after the page, so the last field is the one in front
  const focusDraft = id => { const all = document.querySelectorAll(`[data-draft="${CSS.escape(id)}"]`); if (all.length) all[all.length - 1].focus(); };
  async function setFeedback(el, v) {
    const id = el.dataset.item;
    const value = S.feedback[id] && S.feedback[id].value === v ? null : v;
    noteFor = value === 'down' || value === 'later' ? id : null;
    const msg = value === 'down' ? 'Ausgeblendet. Die Agents schlagen das nicht mehr vor.' : value === 'later' ? `Kommt in ${S.snooze_days} Tagen wieder.` : null;
    if (await react(id, value, undefined, msg)) focusDraft(id); else noteFor = null;
  }
  async function saveNote(id) {
    const text = (drafts[id] || '').trim();
    noteFor = null;
    delete drafts[id];
    if (!text || !S.feedback[id]) { render(); return; }
    if (await react(id, S.feedback[id].value, text)) toast('Gemerkt. Die Agents lesen das beim nächsten Lauf.');
  }
  async function saveAnswer(id) {
    const text = (drafts[id] || '').trim();
    if (!text) { focusDraft(id); return; }
    answering = null;
    if (await react(id, 'answered', text, 'Beantwortet. Der nächste Lauf nimmt deine Antwort in die Karte auf.')) delete drafts[id];
  }
  async function setPref(key, value) { S.prefs[key] = value; render(); try { await call('set_pref', { key, value }); } catch (e) { toast('Einstellung nicht gespeichert.'); } }
  async function setStatus(status, note) {
    try { await call('set_status', { id: openId, status, note }); await load(false); }
    catch (e) { toast('Status nicht gespeichert.'); }
  }

  function rerenderDisc(target = disc && disc.target) { if (target === 'panel') renderPanel(true); else render(); }
  async function pick(target) {
    let path;
    try { path = await call('pick_folder'); } catch (e) { toast(errText(e)); return; }
    if (!path) return;
    disc = { target, busy: 'Suche Git-Repos …' };
    rerenderDisc();
    try {
      const result = await call('discover', { root: path });
      disc = { target, result, checked: new Set(result.candidates.filter(c => c.recommended).map(c => c.path)), busy: null };
    } catch (e) { disc = null; toast(errText(e)); }
    rerenderDisc();
  }
  async function addPaths(paths, target) {
    disc = { ...(disc || { target, checked: new Set() }), busy: paths.length === 1 ? 'Füge hinzu und messe …' : `Füge ${paths.length} Projekte hinzu und messe …` };
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
    if (t.closest('[data-since-close]')) { if (since) since.dismissed = true; if (openId) openDrawer(openId, true); return; }
    if (t.closest('[data-recent-all]')) { showAllRecent = !showAllRecent; render(); return; }
    const ns = t.closest('[data-note-save]');
    if (ns) { saveNote(ns.dataset.noteSave); return; }
    if (t.closest('[data-note-skip]')) { if (noteFor) delete drafts[noteFor]; noteFor = null; render(); return; }
    const an = t.closest('[data-answer]');
    if (an) { answering = an.dataset.answer; render(); focusDraft(answering); return; }
    const as = t.closest('[data-answer-save]');
    if (as) { saveAnswer(as.dataset.answerSave); return; }
    if (t.closest('[data-answer-cancel]')) { answering = null; render(); return; }
    const qd = t.closest('[data-qdone]');
    if (qd) { react(qd.dataset.qdone, 'done', null, 'Als erledigt markiert.'); return; }
    const ru = t.closest('[data-reply-undo]');
    if (ru) { react(ru.dataset.replyUndo, null); return; }
    const fd = t.closest('[data-focus-done]');
    if (fd) { const id = fd.dataset.focusDone; react(id, reply(id) ? null : 'done', null); return; }
    const st = t.closest('[data-status]');
    if (st && openId) { setStatus(st.dataset.status, $('#status-note').value); return; }
    if (t.closest('[data-open-settings]')) { openPanel(); return; }
    if (t.closest('[data-close-panel]')) { closePanel(); return; }
    const pk = t.closest('[data-pick]');
    if (pk) { pick(pk.dataset.target); return; }
    if (t.closest('[data-disc-all]')) { const all = disc.result.candidates; disc.checked = disc.checked.size === all.length ? new Set() : new Set(all.map(c => c.path)); rerenderDisc(); return; }
    if (t.closest('[data-disc-add]')) { addPaths([...disc.checked], disc.target); return; }
    if (t.closest('[data-disc-cancel]')) { const target = disc.target; disc = null; rerenderDisc(target); return; }
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
  // toggle does not bubble, so listen while it travels down
  document.addEventListener('toggle', e => { if (e.target.matches && e.target.matches('[data-hb]')) hbOpen = e.target.open; }, true);
  document.addEventListener('input', e => { if (e.target.matches('[data-draft]')) drafts[e.target.dataset.draft] = e.target.value; });
  document.addEventListener('keydown', e => {
    const draft = e.target.closest && e.target.closest('[data-draft]');
    if (draft && (e.key === 'Enter' || e.key === 'Escape')) {
      e.preventDefault();
      const id = draft.dataset.draft, note = draft.dataset.kind === 'note';
      if (e.key === 'Enter') { if (note) saveNote(id); else saveAnswer(id); }
      else { if (note) { delete drafts[id]; noteFor = null; } else answering = null; render(); }
      return;
    }
    if (e.key === 'Escape') { if (panelOpen) closePanel(); else closeDrawer(); }
  });
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
