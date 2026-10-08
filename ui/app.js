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
  const expanded = { ideas: false };
  let noteFor = null;     // item whose "why?" field shows right after a later/down rating
  let snoozeFor = null;
  let snoozeDate = '';
  let warningFor = null;
  const warningDrafts = {};
  const busyTasks = new Set();
  let answering = null;   // question whose answer field is open
  const drafts = {};      // unsent note and answer text per item, kept across re-renders
  let ideaDraft = '';
  const projectDrafts = {}; // unsaved status note and goal fields per project
  let openId = null;      // project whose drawer is open
  let hbOpen = false;     // the drawer's handbook section is expanded
  let since = null;       // { id, data, dismissed }: what happened since the owner last opened that project
  let panelOpen = false;  // settings panel
  let settings = null;    // autostart, agent access, paths
  const projectEdits = {}; // unsaved names and folder paths in settings
  let disc = null;        // project search: { target, result (from discover), checked:Set, busy }
  let confirmRemove = null;
  let round = null;       // decision round: { queue: question ids, i, stats, busy }
  let searchOpen = false;
  let searchQ = '';
  let searchSel = 0;
  const TASKS = {};       // "Auftrag kopieren" buttons: key → the task they copy, refilled on every render
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
  const projectValue = (p, field) => (projectDrafts[p.id] || {})[field] ?? p[field] ?? '';
  const clearProjectDraft = (id, field, saved) => {
    const d = projectDrafts[id];
    if (d && d[field] === saved) delete d[field];
  };
  function inputFocus() {
    const el = document.activeElement;
    if (!el || el.tagName !== 'INPUT') return null;
    const selector = el.id ? `#${CSS.escape(el.id)}` : el.dataset.draft ? `[data-draft="${CSS.escape(el.dataset.draft)}"]` : el.dataset.warningNote ? `[data-warning-note="${CSS.escape(el.dataset.warningNote)}"]` : null;
    return selector ? { selector, start: el.selectionStart, end: el.selectionEnd } : null;
  }
  function restoreInputFocus(saved) {
    if (!saved) return;
    const fields = document.querySelectorAll(saved.selector), el = fields[fields.length - 1];
    if (el) {
      el.focus({ preventScroll: true });
      if (saved.start != null) el.setSelectionRange(saved.start, saved.end);
    }
  }

  const visible = () => S.projects.filter(p => p.status !== 'archived');
  const lastWork = p => (p.facts && p.facts.last_work) || (p.card_updated_at || '').slice(0, 10) || null;
  const byLastWork = list => [...list].sort((a, b) => (lastWork(b) || '').localeCompare(lastWork(a) || ''));

  function rated(id) {
    const f = S.feedback[id];
    if (!f) return null;
    if (f.value === 'later' && snoozedUntil(f) <= S.today) return null;
    return f.value;
  }
  function addDays(date, days) {
    const d = new Date(date + 'T12:00:00Z');
    d.setUTCDate(d.getUTCDate() + days);
    return d.toISOString().slice(0, 10);
  }
  const snoozedUntil = f => f.snoozed_until || addDays(String(f.at).slice(0, 10), S.snooze_days || 7);
  const hiddenByRating = id => !showRated && snoozeFor !== id && (rated(id) === 'down' || rated(id) === 'later');
  // the owner's reply to a question: answered (with the answer as note) or done
  const reply = id => { const f = id && S.feedback[id]; return f && (f.value === 'answered' || f.value === 'done') ? f : null; };
  const openQs = c => c.waiting_on_you.filter(w => !reply(w.id));
  function findQ(id) {
    for (const p of S.projects) { const w = p.card && p.card.waiting_on_you.find(x => x.id === id); if (w) return { p, w }; }
    return null;
  }
  // the owner's goal for a project, with the days left
  function goalLine(p) {
    if (!p.goal) return '';
    const left = p.goal_due ? -daysSince(p.goal_due) : null;
    const when = left == null ? '' : left < 0 ? ` · <span class="bad-t">${-left} ${left === -1 ? 'Tag' : 'Tage'} drüber</span>` : ` · bis ${fmtShort(p.goal_due)} (noch ${left} ${left === 1 ? 'Tag' : 'Tage'})`;
    return `<div class="goal"><span class="faint">Ziel:</span> ${esc(p.goal)}${when}</div>`;
  }

  // ---------- facts-derived safety ----------
  const acknowledged = (p, key) => !!(p.warnings || []).find(w => w.key === key && w.acknowledged);
  function noRemoteTag(p) {
    return acknowledged(p, 'no_remote') ? '<span class="tag">bewusst ohne Remote</span>' : '<span class="tag bad">kein Remote</span>';
  }
  function warningControls(p) {
    const warnings = p.warnings || [];
    if (!warnings.length) return '';
    const row = w => {
      const key = `${p.id}:${w.key}`;
      const control = w.can_acknowledge === false ? '' : w.acknowledged
        ? `<span>${esc(w.note)}</span><button class="linkish" type="button" data-warning-clear="${esc(w.key)}" data-project="${esc(p.id)}">wieder warnen</button>`
        : warningFor === key ? `<div class="inrow"><input type="text" data-warning-note="${esc(key)}" aria-label="Warum ist das bewusst so?" maxlength="300" value="${esc(warningDrafts[key] || '')}" placeholder="z. B. bewusst lokal, kein GitHub-Projekt"><button class="btn" type="button" data-warning-save="${esc(w.key)}" data-project="${esc(p.id)}">Merken</button><button class="linkish" type="button" data-warning-cancel>Abbrechen</button></div>`
          : `<button class="linkish" type="button" data-warning-open="${esc(w.key)}" data-project="${esc(p.id)}">Bewusst so</button>`;
      return `<div class="warning-row"><strong>${esc(w.title || w.kind || w.key)}</strong>${control}</div>`;
    };
    const active = warnings.filter(w => !w.acknowledged), known = warnings.filter(w => w.acknowledged);
    return dmod('Warnungen einordnen', active.map(row).join('') + (known.length ? `<details class="acknowledged-warnings"><summary>${known.length} bewusst akzeptiert</summary>${known.map(row).join('')}</details>` : ''), 'Deine Begründung gilt für diesen Befund. Wenn er sich wesentlich ändert, meldet Watchtower ihn wieder.');
  }
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
  // Keep the stored preference compatible; formerly hidden sections now stay reachable.
  const collapsedMods = () => new Set(S.prefs.hidden_modules || []);
  function mod(id, title, sub, body) {
    MOD_TITLES[id] = title;
    if (!body) return '';
    const collapsed = collapsedMods().has(id), action = collapsed ? 'ausklappen' : 'einklappen';
    return `<section class="mod${collapsed ? ' is-collapsed' : ''}" data-mod="${id}"><div class="mod-head"><div><h2>${esc(title)}</h2>${sub && !collapsed ? `<div class="sub">${esc(sub)}</div>` : ''}</div><button class="mod-toggle" type="button" data-mod-toggle="${id}" aria-expanded="${!collapsed}" aria-controls="mod-${id}-body" aria-label="${esc(title)} ${action}"><span aria-hidden="true">${collapsed ? '▸' : '▾'}</span> ${action}</button></div><div class="mod-body" id="mod-${id}-body"${collapsed ? ' hidden' : ''}>${body}</div></section>`;
  }
  const dmod = (title, body, sub) => body ? `<section class="mod"><div class="mod-head"><div><h2>${esc(title)}</h2>${sub ? `<div class="sub">${esc(sub)}</div>` : ''}</div></div>${body}</section>` : '';

  function fbButtons(id) {
    if (!id) return '';
    const cur = rated(id);
    const buttons = `<span class="fb" data-item="${esc(id)}">${[['up', 'gut'], ['later', 'später'], ['down', 'Quatsch']].map(([v, l]) => `<button type="button" data-v="${v}" aria-pressed="${cur === v}">${l}</button>`).join('')}</span>`;
    return buttons + snoozeControls(id);
  }
  function snoozeControls(id) {
    const until = rated(id) === 'later' ? `<span class="faint">Wieder am ${fmtDate(snoozedUntil(S.feedback[id]))} · <button class="linkish" type="button" data-snooze-open="${esc(id)}">Datum ändern</button> · <button class="linkish" type="button" data-snooze-clear="${esc(id)}">jetzt zeigen</button></span>` : '';
    const picker = snoozeFor === id ? `<div class="snooze-picker"><span>Wann möchtest du das wieder sehen?</span><div class="snooze-options"><button class="btn" type="button" data-snooze-days="1">Morgen</button><button class="btn" type="button" data-snooze-days="7">In einer Woche</button><input type="date" id="snooze-date" aria-label="Wieder anzeigen am" min="${addDays(S.today, 1)}" value="${esc(snoozeDate)}"><button class="btn primary" type="button" data-snooze-save>Datum merken</button><button class="linkish" type="button" data-snooze-cancel>Abbrechen</button></div></div>` : '';
    return until + picker;
  }

  // ---------- handing a task to a coding agent ----------
  function taskBtn(key, task) {
    const snapshot = { ...task, source_id: key };
    const active = (S.tasks || []).find(t => ['assigned', 'blocked', 'review'].includes(t.status) && t.source_id === key && [...t.projects].sort().join('|') === [...task.projects].sort().join('|'));
    TASKS[key] = active || snapshot;
    return `<span class="task-actions"><button class="linkish task" type="button" data-task="${esc(key)}" title="Kopiert einen fertigen Auftrag für einen Coding-Agent">Auftrag kopieren</button>${active ? `<span class="faint">${esc(TASK_STATUS[active.status])} · #${active.id}</span>` : `<button class="linkish task" type="button" data-task-assign="${esc(key)}" title="Merkt, dass du diesen Auftrag an einen Agent übergibst">Beauftragt</button>`}</span>`;
  }
  function taskPrompt(t) {
    const ps = t.projects.map(id => byId(id) || (t.project_snapshots || []).find(p => p.id === id)).filter(Boolean);
    const lines = [!ps.length ? 'Aufgabe für ein neues Projekt:' : ps.length === 1 ? `Aufgabe im Projekt ${ps[0].name} (${ps[0].path}):` : `Aufgabe über die Projekte ${ps.map(p => `${p.name} (${p.path})`).join(', ')}:`, t.title, ''];
    if (t.why) lines.push(`Warum: ${t.why}`);
    if (t.evidence) lines.push(`Beleg: ${t.evidence}`);
    if (t.done_when) lines.push(`Fertig, wenn: ${t.done_when}`);
    for (const p of ps) {
      const c = p.card || {}, h = c.handbook || {}, pre = ps.length > 1 ? `${p.name}: ` : '';
      if ((c.decisions || []).length) lines.push('', `${pre}Bewusst so entschieden, bitte nicht umwerfen:`, ...c.decisions.map(d => `- ${d}`));
      if ((h.read_first || []).length) lines.push('', `${pre}Zum Einlesen: ${h.read_first.map(r => r.path).join(', ')}`);
      if ((h.run || []).length) lines.push(`${pre}Befehle laut Handbuch:`, ...h.run.map(r => `- ${r.what}: ${r.how}${r.folder && r.folder !== '.' ? ` (in ${r.folder})` : ''}`));
    }
    if (Number.isInteger(t.id)) lines.push('', `Dieser Auftrag wird in Watchtower als #${t.id} verfolgt. Melde das Ergebnis mit konkretem Beleg zurück:`, `watchtower tasks report ${t.id} --status done --result "Was erledigt wurde" --evidence "Prüfung, Datei oder Commit"`, `Wenn du blockiert bist: watchtower tasks report ${t.id} --status blocked --result "Was fehlt und wer es klären kann"`);
    lines.push('', `Aus Watchtower kopiert am ${fmtDate(S.today)}. Prüfe den Befund zuerst gegen den aktuellen Stand; ist er überholt, sag das, statt etwas zu ändern.`);
    return lines.join('\n');
  }

  // The owner explicitly records assignments; copying a prompt alone changes nothing.
  const TASK_STATUS = { assigned: 'Beauftragt', blocked: 'Blockiert', review: 'Zur Prüfung', done: 'Erledigt bestätigt', cancelled: 'Beendet' };
  function trackedTask(t, history = false) {
    const key = `tracked:${t.id}`;
    TASKS[key] = t;
    const identity = `data-task-id="${t.id}" data-task-revision="${t.revision || 1}"`;
    const actions = t.status === 'review'
      ? `<button class="btn primary" type="button" data-task-status="done" ${identity}>Ergebnis bestätigen</button><button class="btn" type="button" data-task-status="assigned" ${identity}>Zurück an den Agent</button>`
      : t.status === 'blocked' ? `<button class="btn" type="button" data-task-status="assigned" ${identity}>Wieder freigeben</button>`
        : ['done', 'cancelled'].includes(t.status) ? `<button class="btn" type="button" data-task-status="assigned" ${identity}>Wieder öffnen</button>` : '';
    const active = ['assigned', 'blocked', 'review'].includes(t.status);
    const events = history && (t.history || []).length ? `<details class="task-history"><summary class="more">Verlauf</summary><ul class="bullets">${t.history.map(h => `<li>${h.actor === 'owner' ? 'Du' : 'Agent'}: ${esc(TASK_STATUS[h.status] || h.status || '')}${h.at ? ` · ${fmtDate(h.at)}` : ''}${h.result ? ` · ${esc(h.result)}` : ''}${h.evidence ? `<div class="ev">${esc(h.evidence)}</div>` : ''}</li>`).join('')}</ul></details>` : '';
    return `<article class="idea tracked-task" data-status="${esc(t.status)}"><div class="h"><div class="t">${esc(t.title)}</div><span class="task-status" data-status="${esc(t.status)}">${esc(TASK_STATUS[t.status] || t.status)}</span></div><div class="who">${(t.projects || []).map(plink).join('')}<span class="faint">#${t.id} · ${ago(t.updated_at || t.created_at)}</span></div>${t.result ? `<div class="task-result">${esc(t.result)}</div>` : ''}${t.result_evidence ? `<div class="ev">${esc(t.result_evidence)}</div>` : ''}${t.done_when ? `<div class="why">Fertig, wenn: ${esc(t.done_when)}</div>` : ''}${t.why || t.evidence ? `<details><summary class="more">Ursprünglicher Auftrag</summary><div class="why">${esc(t.why)}</div><div class="ev">${esc(t.evidence)}</div></details>` : ''}<div class="task-actions">${actions}${active ? `<button class="linkish task" type="button" data-task="${key}">Auftrag kopieren</button><button class="linkish" type="button" data-task-status="cancelled" ${identity}>Beenden</button>` : ''}</div>${events}</article>`;
  }
  function blockTasks(project = null) {
    const all = (S.tasks || []).filter(t => !project || (t.projects || []).includes(project));
    const active = all.filter(t => ['assigned', 'blocked', 'review'].includes(t.status)).sort((a, b) => ({ review: 0, blocked: 1, assigned: 2 }[a.status] - { review: 0, blocked: 1, assigned: 2 }[b.status]));
    const rest = all.filter(t => !active.includes(t));
    if (!all.length || (!active.length && !rest.length)) return '';
    const shownTasks = active.slice(0, 3), remaining = active.slice(3);
    const body = `<div class="cards2">${shownTasks.map(t => trackedTask(t, !!project)).join('')}</div>${remaining.length ? `<details><summary class="more">${remaining.length} weitere Aufträge</summary><div class="cards2">${remaining.map(t => trackedTask(t, !!project)).join('')}</div></details>` : ''}${rest.length ? `<details><summary class="more">${rest.length} abgeschlossene Aufträge</summary><div class="cards2">${rest.map(t => trackedTask(t, true)).join('')}</div></details>` : ''}`;
    return project ? dmod('Aufträge', body) : mod('tasks', 'Deine Aufträge', `${active.filter(t => t.status === 'review').length} zur Prüfung · ${active.filter(t => t.status === 'blocked').length} blockiert · ${active.filter(t => t.status === 'assigned').length} beauftragt`, body);
  }

  function renderRunStatus() {
    const r = S.run_status;
    const control = $('#report-status');
    control.hidden = !r;
    if (!r) return;
    const labels = { unknown: 'Berichte noch ungeprüft', running: 'Berichte werden geprüft', stalled: 'Prüfung unvollständig', incomplete: 'Prüfung unvollständig', failed: 'Prüfung fehlgeschlagen', complete: 'Berichte geprüft' };
    const latest = r.latest || {}, counts = latest.counts || {};
    const pending = Array.isArray(r.pending_owner_replies) ? r.pending_owner_replies.length : Number(r.pending_owner_replies || 0);
    const reasonText = value => {
      if (Array.isArray(value)) return value.map(reasonText).join(', ');
      if (value && typeof value === 'object') return reasonText(value.message || value.code);
      const labels = { no_card: 'Einschätzung fehlt', changed: 'Projekt hat sich geändert', ci_changed: 'CI hat sich geändert', answered: 'Deine Antwort fehlt noch im Bericht', status_changed: 'Deine Vorgaben haben sich geändert', new_fields: 'Einschätzung muss ergänzt werden', stale: 'Einschätzung ist veraltet', missing: 'Projektüberblick fehlt', new_day: 'Projektüberblick für heute fehlt', owner_idea: 'Deine neue Idee muss eingeschätzt werden', cards_changed: 'Projektberichte wurden aktualisiert', cards_will_change: 'Offene Projektberichte müssen zuerst aktualisiert werden', facts_unreadable: 'Fakten konnten nicht gelesen werden', facts_missing: 'Fakten fehlen', path_missing: 'Projektordner fehlt', not_a_git_repository: 'Kein Git-Repository', git_status_failed: 'Git-Status konnte nicht gelesen werden', agent_failed: 'Agent konnte die Prüfung nicht abschließen', project_removed: 'Projekt nicht mehr beobachtet', project_archived: 'Projekt archiviert' };
      return labels[value] || (typeof value === 'string' && !/^[a-z]+(?:_[a-z]+)+$/.test(value) ? value : 'Weitere Prüfung nötig');
    };
    const name = p => byId(p.id) ? plink(p.id) : esc(p.name || p.id);
    const projects = latest.projects || [];
    const completed = p => ['current', 'complete', 'completed', 'written'].includes(p.status);
    const excluded = p => ['project_removed', 'project_archived'].includes(p.reason) || p.status === 'skipped';
    const checked = projects.filter(p => completed(p) && !excluded(p));
    const versionChange = p => {
      const version = p.card_version ?? p.version;
      return Number.isInteger(p.initial_version) && Number.isInteger(version) && version >= p.initial_version && version > 0 ? version - p.initial_version : null;
    };
    const updated = checked.filter(p => versionChange(p) > 0);
    const unchanged = checked.filter(p => versionChange(p) === 0);
    const unspecified = checked.filter(p => versionChange(p) == null);
    const group = (title, list) => list.length ? `<p><b>${title} (${list.length}):</b> ${list.map(name).join(', ')}.</p>` : '';
    const outcomes = group('Berichte aktualisiert', updated) + group('Unverändert übernommen', unchanged)
      + (unspecified.length ? `${group('Berichte geprüft', unspecified)}<p class="faint">Ob diese Berichte überarbeitet wurden, ist nicht erfasst.</p>` : '')
      + (!checked.length && counts.current ? `<p>${counts.current} von ${counts.total || counts.current} Projektberichten waren aktuell. Einzelne Aktualisierungen sind nicht erfasst.</p>` : '');
    const problems = projects.filter(p => !completed(p) && !excluded(p));
    const lines = problems.map(p => `<li>${name(p)}: ${esc(({ pending: 'steht noch aus', failed: 'fehlgeschlagen', changed: 'erneut geändert', running: 'in Arbeit' })[p.status] || 'noch offen')}${p.error || p.reason ? ` · ${esc(reasonText(p.error || p.reason))}` : ''}</li>`).join('');
    const repair = typeof r.repair_task === 'string' ? r.repair_task : r.repair_task && (r.repair_task.prompt || r.repair_task.text);
    const pf = latest.portfolio;
    const portfolioText = !pf ? '' : completed(pf)
      ? versionChange(pf) > 0 || pf.status === 'written' ? 'Projektüberblick neu geschrieben. Er bündelt Prioritäten, Verbindungen und Ideen.'
        : versionChange(pf) === 0 ? 'Projektüberblick geprüft und unverändert übernommen.'
          : 'Projektüberblick geprüft. Ob er überarbeitet wurde, ist nicht erfasst.'
      : `Projektüberblick ${pf.status === 'failed' ? 'konnte nicht aktualisiert werden' : 'steht noch aus'}${pf.error || pf.reason ? `: ${reasonText(pf.error || pf.reason)}` : '.'}`;
    const timestamp = value => {
      const date = new Date(value);
      return value && Number.isFinite(date.getTime()) ? `${date.toLocaleString('de-DE', { day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit' })} Uhr` : null;
    };
    const finished = timestamp(r.last_successful && r.last_successful.finished_at || (r.state === 'complete' && latest.finished_at));
    const started = timestamp(latest.started_at);
    const timing = finished ? `Letzte vollständige Prüfung: ${finished}.` : r.state === 'complete' ? 'Prüfung abgeschlossen; Abschlusszeit nicht erfasst.' : 'Noch keine vollständige Prüfung erfasst.';
    // Keep the native details element itself in place so refresh preserves open state and summary focus.
    control.setAttribute('data-state', pending && r.state === 'complete' ? 'incomplete' : r.state || 'unknown');
    $('#report-status-summary').textContent = `${labels[r.state] || labels.unknown}${pending ? ` · ${pending} ${pending === 1 ? 'Antwort offen' : 'Antworten offen'}` : ''}`;
    $('#report-status-body').innerHTML = `<p class="faint">${esc(timing)}</p>${r.state !== 'complete' && started ? `<p>Aktuelle Prüfung begonnen: ${esc(started)}.</p>` : ''}${outcomes}${portfolioText ? `<p>${esc(portfolioText)}</p>` : ''}${lines ? `<ul class="report-status-problems">${lines}</ul>` : ''}${pending ? `<p>${pending} ${pending === 1 ? 'Antwort wartet' : 'Antworten warten'} noch darauf, in die Projektberichte übernommen zu werden.</p>` : ''}${r.state === 'unknown' ? '<p>Ein Agent prüft die Projekte und schreibt bei Bedarf neue Berichte. „Aktualisieren“ misst nur die Git-Fakten.</p>' : ''}${repair ? `<button class="btn" type="button" data-copy="${esc(repair)}">Auftrag zum Fortsetzen kopieren</button>` : ''}`;
  }

  function blockChanges() {
    const c = S.changes;
    if (!c) return '';
    const entries = c.entries || [];
    if (c.has_baseline && !entries.length && !collapsedMods().has('changes')) return `<p class="digest-empty faint">Keine neuen Projektmeldungen seit deinem Lesestand${c.since ? ` vom ${fmtDate(c.since)}` : ''}.</p>`;
    const row = e => `<div class="change-item"><div class="h"><span class="change-kind">${esc(({ new: 'Neu', resolved: 'Nicht mehr offen', decision_applied: 'Antwort übernommen' })[e.kind] || e.kind)}</span>${e.project ? plink(e.project) : ''}</div><div class="t">${esc(e.title)}</div>${e.detail ? `<div class="why">${esc(e.detail)}</div>` : ''}${e.next_step ? `<div class="why"><b>Nächster Schritt:</b> ${esc(e.next_step)}</div>` : ''}</div>`;
    const body = entries.length ? `<div class="digest-items">${entries.slice(0, 3).map(row).join('')}</div>${entries.length > 3 ? `<details><summary class="more">${entries.length - 3} weitere Änderungen</summary><div class="digest-items">${entries.slice(3).map(row).join('')}</div></details>` : ''}` : `<p class="faint">${c.has_baseline ? 'Keine neuen Projektmeldungen seit deinem letzten Lesestand.' : 'Markiere den aktuellen Stand als gelesen. Danach siehst du hier neue Fortschritte, offene Punkte, Fragen und Entscheidungen aus den Projektberichten.'}</p>`;
    const action = c.token && (entries.length || !c.has_baseline) ? `<div class="actions"><button class="btn" type="button" data-changes-read="${esc(c.token)}">${c.has_baseline ? 'Diese Änderungen als gelesen markieren' : 'Aktuellen Stand als gelesen markieren'}</button></div>` : '';
    return mod('changes', 'Seit deinem letzten Lesen', `Fortschritte, offene Punkte und Entscheidungen${c.since ? ` · Lesestand vom ${fmtDate(c.since)}` : ''}`, `<div class="digest-summary">${body}${action}</div>`);
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
      ${snoozeControls(id)}<div class="why">Magst du kurz sagen, warum? Die Agents richten künftige Vorschläge danach.</div>
      <div class="inrow"><input type="text" aria-label="Warum" data-draft="${esc(id)}" data-kind="note" maxlength="300" value="${esc(drafts[id] || '')}" placeholder="${down ? 'z. B. passt nicht zum Projekt' : 'z. B. erst nach dem Release'}">
      <button type="button" class="btn primary" data-note-save="${esc(id)}">Merken</button><button type="button" class="btn" data-note-skip>Überspringen</button></div></div>`;
  }
  const myNote = id => { const f = S.feedback[id]; return f && f.note && !reply(id) ? `<div class="mynote">Deine Notiz: ${esc(f.note)}</div>` : ''; };

  function ideaCard(s, projects, projectId, kind) {
    if (noteFor === s.id) return noteForm(s.id, s.title);
    if (hiddenByRating(s.id)) return '';
    return `<div class="idea">${kind ? `<div class="kind">${kind}</div>` : ''}<div class="h"><div class="t">${s.severity ? `<span class="sev-${esc(s.severity)}">●</span> ` : ''}${esc(s.title)}</div></div>
      <div class="why">${esc(s.why || s.pitch)}</div>
      ${s.because ? `<div class="why"><b>Weil:</b> ${esc(s.because)}</div>` : ''}
      ${s.evidence ? `<details><summary class="more">Beleg</summary><div class="ev">${esc(s.evidence)}</div></details>` : ''}
      ${myNote(s.id)}
      <div class="foot"><div class="who">${(projects || []).map(plink).join('')}${s.type ? `<span class="tag">${esc(TYPE[s.type] || s.type)}</span>` : ''}</div>${fbButtons(s.id)}</div>
      <div class="taskrow">${taskBtn(s.id || s.title, { projects: projectId ? [projectId] : (projects || []), title: s.title, why: s.why || s.pitch, evidence: s.evidence, done_when: s.done_when })}</div></div>`;
  }

  function blockFocus() {
    const t = (S.portfolio && S.portfolio.focus) || [];
    if (!t.length) return '';
    return mod('focus', 'Heute zuerst', 'Über alle Projekte hinweg ausgewählt', `<div class="focus">${t.slice(0, 3).map((it, i) => `
      <div class="item${reply(it.id) ? ' done' : ''}"><div class="n">${i + 1}</div><div><div class="t">${esc(it.title)}</div>
      <div class="meta">${plink(it.project)}${taskBtn(it.id || `focus-${i}`, { projects: [it.project], title: it.title, why: it.why, done_when: it.done_when })}${it.id ? `<button type="button" class="check" data-focus-done="${esc(it.id)}" aria-pressed="${!!reply(it.id)}">${reply(it.id) ? '✓ erledigt' : 'erledigt?'}</button>` : ''}</div>
      <div class="why">${esc(it.why)}</div></div></div>`).join('')}</div>`);
  }

  // what the facts say about a project, as a few short badges
  const bytes = n => n >= 1e9 ? `${(n / 1e9).toLocaleString('de-DE', { maximumFractionDigits: 1 })} GB` : `${Math.round(n / 1e6).toLocaleString('de-DE')} MB`;
  const plural = (n, one, many) => `${num(n)} ${n === 1 ? one : many}`;
  // two lines of work that change the same files may conflict when merged
  const lineName = l => l.kind === 'main' ? 'ungesicherte Arbeit im Hauptordner' : l.kind === 'worktree' ? `Worktree ${l.name}` : `Branch ${l.name}`;
  const collisions = p => (p.facts && p.facts.collisions) || [];
  const collisionText = c => `${lineName(c.a)} und ${lineName(c.b)} ändern ${plural(c.count, 'gleiche Datei', 'gleiche Dateien')}: ${c.files.join(', ')}${c.count > c.files.length ? ' …' : ''}`;
  function collisionTag(p) {
    const list = collisions(p);
    return list.length ? `<span class="tag warn" title="${esc(list.map(collisionText).join('\n'))}">${list.length === 1 ? 'Überschneidung' : `${list.length} Überschneidungen`}</span>` : '';
  }
  const bareVersion = v => String(v || '').trim().replace(/^v/i, '');
  // the newest tag, the work since, and whether the installed copy is older
  function releaseText(p) {
    const r = p.facts && p.facts.release;
    if (!r) return null;
    const installed = p.card && p.card.handbook && p.card.handbook.installed;
    const inst = installed && installed.version && bareVersion(installed.version) !== bareVersion(r.tag) ? ` · installiert ist ${installed.version}` : '';
    return `${r.tag}${r.date ? ` vom ${fmtDate(r.date)}` : ''} · ${r.commits_since ? `${plural(r.commits_since, 'Commit', 'Commits')} seither` : 'nichts seither'}${inst}`;
  }
  const unreleasedTag = p => { const r = p.facts && p.facts.release; return r && r.commits_since >= 10 ? `<span class="tag" title="${esc(releaseText(p))}">${num(r.commits_since)} seit ${esc(r.tag)}</span>` : ''; };

  function signals(p) {
    const f = p.facts || {};
    if (!f.is_git) return `<span class="tag">${f.error === 'path_missing' ? 'Ordner fehlt' : 'kein Git'}</span>`;
    const s = safety(p);
    const out = [];
    if (!s.remote) out.push(noRemoteTag(p));
    if (s.unsavedShort) out.push(`<span class="tag ${s.level}">${esc(s.unsavedShort)}</span>`);
    if (s.ahead) out.push(`<span class="tag warn">${s.ahead} ungepusht</span>`);
    if (s.ci === 'red') out.push('<span class="tag bad">CI rot</span>');
    if (!out.length) out.push('<span class="tag good">gesichert</span>');
    out.push(collisionTag(p));
    return out.join('');
  }
  const risk = p => { const s = safety(p); return (p.facts && p.facts.is_git && !s.remote && !acknowledged(p, 'no_remote') ? 2 : 0) + (s.level === 'bad' ? 2 : s.level === 'warn' ? 1 : 0) + (s.ci === 'red' ? 1 : 0) + (s.ahead ? 1 : 0); };

  function blockProjects() {
    const list = [...visible()].sort((a, b) => risk(b) - risk(a) || (lastWork(b) || '').localeCompare(lastWork(a) || ''));
    if (!list.length) return '';
    const tile = p => {
      const c = p.card, waiting = c ? openQs(c).length : 0;
      const fresh = isNew(p.card_updated_at) ? '<span class="pill new">neu</span>' : '';
      return `<button class="tile" type="button" data-open="${esc(p.id)}">
        <div class="h"><span class="name">${dot(p.id)}${esc(p.name)}</span><span class="pills">${fresh}${p.status !== 'active' ? `<span class="pill unknown">${esc(STATUS[p.status])}</span>` : phasePill(p)}</span></div>
        <p class="voice">${c ? `„${esc(c.personality || c.headline)}“` : '<span class="faint">Noch keine Einschätzung. Kommt beim nächsten Agent-Lauf.</span>'}</p>
        ${goalLine(p)}
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
    const opts = (w.options || []).length && w.id && answering !== w.id ? `<div class="qopts">${w.options.map((o, i) => `<button type="button" class="opt" data-opt="${esc(w.id)}" data-i="${i}" title="${esc(o.effect || '')}">${esc(o.answer)}</button>`).join('')}</div>` : '';
    const src = w.source ? (withSource ? `<div class="qsrc mono">${esc(w.source)}</div>` : '') : '';
    return `<li class="q"${w.source && !withSource ? ` title="Quelle: ${esc(w.source)}"` : ''}><div class="qrow"><span class="qt">${esc(w.title)}</span>${acts}</div>${opts}${src}${form}</li>`;
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
    const start = total > 1 ? `<div class="round-start"><button class="btn primary" type="button" data-round-start>Entscheidungsrunde: alle ${total} nacheinander</button></div>` : '';
    return mod('waiting', 'Wartet auf dich', `${total} ${total === 1 ? 'Entscheidung' : 'Entscheidungen'}, die nur du treffen kannst. Deine Antworten lesen die Agents beim nächsten Lauf`, `${start}<div class="groups">${body}</div>`);
  }

  function blockRecent(limit = 10) {
    const items = visible().flatMap(p => (p.card ? p.card.recent_changes : []).map(c => ({ ...c, project: p.id, reportedAt: p.card_updated_at || '' }))).filter(c => daysSince(c.date) <= 14).sort((a, b) => b.date.localeCompare(a.date) || b.reportedAt.localeCompare(a.reportedAt));
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
    const more = items.length > limit ? `<div class="recent-controls"><span class="faint">${showAllRecent ? `${items.length} Einträge` : `${limit} neueste von ${items.length}`}</span><button class="more" type="button" data-recent-all>${showAllRecent ? `nur die neuesten ${limit} anzeigen` : `alle ${items.length} anzeigen`}</button></div>` : '';
    return mod('recent', 'Was passiert ist', 'Letzte 14 Tage, neueste zuerst. Am selben Tag stehen zuletzt aktualisierte Karten oben. Klick zeigt Details', `${more}<div class="events">${body}</div>`);
  }

  function blockSafety() {
    const rows = byLastWork(visible()).filter(p => p.facts && p.facts.is_git).map(p => {
      const s = safety(p);
      const unsaved = [s.unsaved ? `<span class="tag ${s.level}">${esc(s.unsaved)}</span>` : '', s.ahead ? `<span class="tag warn">${s.ahead} ungepusht</span>` : ''].filter(Boolean).join(' ');
      const ci = CI[s.ci] || CI.unknown;
      return `<tr><td>${plink(p.id)}</td>
        <td>${s.remote ? `<span class="tag good">${esc(p.facts.remotes.map(r => r.name).join(', '))}</span>` : noRemoteTag(p)}</td>
        <td>${unsaved || '<span class="faint">sauber</span>'}</td>
        <td>${s.leftovers ? `<span class="faint">${esc(s.leftovers)}</span>` : ''} ${collisionTag(p)}</td>
        <td>${ci[1] ? `<span class="tag ${ci[1]}">${ci[0]}</span>` : `<span class="faint">${ci[0]}</span>`}</td></tr>`;
    }).join('');
    if (!rows) return '';
    return mod('safety', 'Sicherheitslage', 'Was verloren ginge, wenn heute die Platte stirbt oder ein Agent falsch auscheckt', `<div class="tablewrap"><table class="safety"><thead><tr><th>Projekt</th><th>Backup</th><th>Ungesichert</th><th>Liegengeblieben</th><th>CI</th></tr></thead><tbody>${rows}</tbody></table></div>`);
  }

  // what the agents did overnight, with the signs that something went wrong
  function blockNight() {
    const rows = byLastWork(visible()).filter(p => p.facts && p.facts.night && (p.facts.night.commits || p.facts.night.uncommitted_touched)).map(p => {
      const n = p.facts.night;
      const warn = [n.reverts ? `<span class="tag warn">${plural(n.reverts, 'Revert', 'Reverts')}</span>` : '', (p.facts.ci || {}).state === 'red' ? '<span class="tag bad">CI rot</span>' : '', collisionTag(p)].join('');
      const what = [n.commits ? `${plural(n.commits, 'Commit', 'Commits')}, +${num(n.lines_added)}/−${num(n.lines_removed)} Zeilen` : '', n.uncommitted_touched ? `${plural(n.uncommitted_touched, 'Datei', 'Dateien')} ohne Commit bearbeitet` : ''].filter(Boolean).join(' · ');
      return `<div class="row"><div class="t">${plink(p.id)}</div><div class="badges">${warn}</div><div class="d">${what}</div></div>`;
    }).join('');
    return mod('night', 'Seit gestern Abend', rows ? 'Was seit gestern 18 Uhr passiert ist, über alle Branches und Worktrees' : null, rows ? `<div class="list">${rows}</div>` : '<p class="faint">Seit gestern 18 Uhr war Ruhe.</p>');
  }

  // build output and installed packages that can be recreated; freeing them is safe, most of all in resting projects
  function blockSpace() {
    const list = visible().filter(p => p.facts && p.facts.space && p.facts.space.total_bytes >= 1e8).sort((a, b) => b.facts.space.total_bytes - a.facts.space.total_bytes);
    const total = list.reduce((a, p) => a + p.facts.space.total_bytes, 0);
    if (total < 1e9) return '';
    const rows = list.map(p => {
      const sp = p.facts.space, phase = p.card && p.card.phase;
      const resting = phase === 'dormant' || phase === 'done' || p.status === 'parked' || p.status === 'done';
      const dirs = sp.dirs.filter(d => d.bytes >= 1e6).slice(0, 4).map(d => `<button class="linkish mono" type="button" data-external="${esc(p.path + '\\' + d.path.replace(/\//g, '\\'))}" title="Im Explorer öffnen">${esc(d.path)}</button> ${bytes(d.bytes)}`).join(' · ');
      return `<div class="row${resting ? ' resting' : ''}"><div class="t">${plink(p.id)}${resting ? ' <span class="tag good">ruht, gefahrlos frei</span>' : ''}</div><div class="mono">${bytes(sp.total_bytes)}</div><div class="d">${dirs}</div></div>`;
    }).join('');
    return mod('space', 'Platz', `${bytes(total)} in Build- und Paketordnern, die sich neu erzeugen lassen. Einmal am Tag gemessen`, `<div class="list">${rows}</div>`);
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
    let total = 0, canExpand = false;
    const fromProjects = visible().flatMap(p => {
      const list = ((p.card || {}).suggestions || []).filter(s => s.id === noteFor || !hiddenByRating(s.id));
      total += list.length;
      canExpand ||= list.length > 1;
      const first = list.find(x => x.id === noteFor || x.id === snoozeFor) || list[0];
      return (expanded.ideas ? list : first ? [first] : []).map(s => ideaCard(s, [p.id], p.id, 'Aus dem Projekt'));
    });
    const apps = ((S.portfolio || {}).new_apps || []).map(s => ideaCard(s, [], null, 'Neue App')).filter(Boolean);
    total += apps.length;
    const cards = [...fromProjects, ...apps].filter(Boolean);
    const count = cards.length < total ? `${cards.length} von ${total} Vorschlägen` : `${total} ${total === 1 ? 'Vorschlag' : 'Vorschläge'}`;
    const control = `<div class="recent-controls"><span class="faint">${count}</span>${canExpand ? `<button class="more" type="button" data-expand="ideas" aria-expanded="${expanded.ideas}" aria-controls="ideas-list">${expanded.ideas ? 'wieder einen pro Projekt anzeigen' : `alle ${total} anzeigen`}</button>` : ''}</div>`;
    return cards.length ? mod('ideas', 'Ideen', 'Vorschläge aus deinen Projekten und neue App-Ideen', `${control}<div class="cards3" id="ideas-list">${cards.join('')}</div>`) : '';
  }

  // ideas the owner parks; the next brief says whether and where each one is worth doing
  const IDEA_VERDICT = { build: ['eigene App', 'good'], fold_in: ['in Projekt einbauen', 'good'], later: ['später', ''], drop: ['lieber nicht', 'bad'] };
  function blockParking() {
    const checks = (S.portfolio || {}).idea_checks || [];
    const items = [...(S.ideas || [])].reverse().map(i => {
      const c = checks.find(x => x.idea === i.id);
      const v = c && (IDEA_VERDICT[c.verdict] || [c.verdict, '']);
      const verdict = c
        ? `<div class="h"><span class="tag ${v[1]}">${esc(v[0])}</span></div><div class="why">${esc(c.why)}</div>${(c.fits || []).length ? `<div class="who">${c.fits.map(plink).join('')}</div>` : ''}`
        : '<div class="faint" style="font-size:13px">Der nächste Agent-Lauf schätzt sie ein.</div>';
      const task = c && (c.verdict === 'build' || c.verdict === 'fold_in') ? taskBtn(`idea:${i.id}`, { projects: c.fits || [], title: i.text, why: c.why }) : '';
      return `<div class="idea"><div class="h"><div class="t">${esc(i.text)}</div><button class="linkish faint" type="button" data-idea-remove="${i.id}" title="Idee entfernen" aria-label="Idee entfernen">entfernen</button></div>${verdict}<div class="foot"><span class="faint mono" style="font-size:12px">geparkt ${fmtShort(i.added_at)}</span>${task}</div></div>`;
    }).join('');
    const form = `<div class="parkform"><input id="idea-text" type="text" value="${esc(ideaDraft)}" maxlength="600" placeholder="Idee festhalten, z. B. eine App, die meine Stromzähler ausliest" aria-label="Neue Idee"><button class="btn" type="button" data-idea-add>Parken</button></div>`;
    return mod('parking', 'Ideen-Parkplatz', 'Halt eine Idee fest; der nächste Agent-Lauf prüft, ob und wo sie sich lohnt', form + (items ? `<div class="cards2">${items}</div>` : ''));
  }

  // the first brief of a week looks back on the week before; shown until the next one
  function blockWeek() {
    const r = (S.portfolio || {}).week_review;
    if (!r || r.week !== S.review_week) return '';
    const list = xs => (xs || []).length ? `<ul class="bullets">${xs.map(x => `<li>${esc(x)}</li>`).join('')}</ul>` : '<p class="faint">nichts</p>';
    return mod('week', `Rückblick KW ${Number(r.week.split('-W')[1])}`, r.time_note || null, `<div class="weekgrid"><div><h3>Geschafft</h3>${list(r.done)}</div><div><h3>Liegengeblieben</h3>${list(r.stalled)}</div></div>`);
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
    const aside = blockWeek() + blockNight() + blockRecent() + blockPulse();
    return `<div class="handover">
      <div class="greet"><div class="kicker">Übergabe · ${n} ${n === 1 ? 'Projekt' : 'Projekte'} ${fresh}</div><h2>${esc(greeting)}</h2></div>
      ${missingCardsNote()}
      ${blockFocus()}${blockProjects()}
      <div class="duo">${blockWaiting()}${aside ? `<div class="aside">${aside}</div>` : ''}</div>
      ${blockAcross()}${blockIdeas()}${blockParking()}
    </div>`;
  }

  // ---------- board ----------
  function card(p) {
    const s = safety(p), c = p.card;
    const badges = [
      p.facts && p.facts.is_git && !s.remote ? noRemoteTag(p) : '',
      s.unsaved ? `<span class="tag ${s.level}">${num(p.facts.dirty.files)} Dateien offen</span>` : '',
      s.ahead ? `<span class="tag warn">${s.ahead} ungepusht</span>` : '',
      s.ci === 'red' ? '<span class="tag bad">CI rot</span>' : '',
      collisionTag(p),
      unreleasedTag(p),
      p.status !== 'active' ? `<span class="tag">${esc(STATUS[p.status])}</span>` : '',
      p.card_outdated ? '<span class="tag">seit Karte geändert</span>' : '',
    ].join('');
    const fresh = isNew(p.card_updated_at) ? '<span class="pill new">neu</span>' : '';
    const commits = (p.facts && p.facts.commits) || { last_30d: 0 };
    return `<button class="card" type="button" data-open="${esc(p.id)}">
      <div class="h"><span class="name">${dot(p.id)}${esc(p.name)}</span><span style="display:flex;gap:6px">${fresh}${phasePill(p)}</span></div>
      <div class="hl">${c ? esc(c.headline) : '<span class="faint">Noch keine Einschätzung. Kommt beim nächsten Agent-Lauf.</span>'}</div>
      ${goalLine(p)}
      ${spark(weekly(p), color(p.id))}
      <div class="stats"><span>zuletzt ${ago(lastWork(p))}</span><span class="mono">${num(commits.last_30d)} / 30 T</span></div>
      ${badges ? `<div class="badges">${badges}</div>` : ''}
      ${c ? `<div class="next"><span class="faint" style="font-size:11.5px;letter-spacing:.06em;text-transform:uppercase">Wiedereinstieg</span><b>${esc(c.reentry.next_step)}</b><span>${openQs(c).length} Entscheidungen · ${c.open_ends.length} offene Enden</span></div>` : ''}
    </button>`;
  }

  function renderBoard() {
    const list = visible();
    const git = list.filter(p => p.facts && p.facts.is_git);
    const noRemote = git.filter(p => !p.facts.has_remote && !acknowledged(p, 'no_remote')).length;
    const unsaved = git.filter(p => (p.facts.dirty || {}).files || p.facts.ahead).length;
    const red = git.filter(p => (p.facts.ci || {}).state === 'red').length;
    const waiting = list.reduce((a, p) => a + (p.card ? openQs(p.card).length : 0), 0);
    const kpis = `<div class="kpis">
      <div class="kpi ${noRemote ? 'bad' : 'ok'}"><div class="v">${noRemote}</div><div class="l">Remote ungeklärt</div></div>
      <div class="kpi ${unsaved ? 'warn' : 'ok'}"><div class="v">${unsaved}</div><div class="l">mit ungesicherter Arbeit</div></div>
      <div class="kpi ${red ? 'bad' : 'ok'}"><div class="v">${red}</div><div class="l">mit rotem CI</div></div>
      <div class="kpi"><div class="v">${waiting}</div><div class="l">Entscheidungen offen</div></div></div>`;
    const grid = mod('cards', 'Projekte', 'Sortiert nach letzter Arbeit. Klick öffnet den Steckbrief', `<div class="grid">${byLastWork(list).map(card).join('')}</div>`);
    return `${mod('kpis', 'Lage auf einen Blick', null, kpis)}
      <div class="board"><div>${grid}${blockSafety()}${blockRecent()}${blockPulse()}${blockSpace()}</div>
      <aside class="side">${blockFocus()}${blockWeek()}${blockNight()}${blockWaiting(1)}${blockParking()}${blockAcross()}</aside></div>`;
  }

  // ---------- handbook ----------
  // how to start a project, how it is built, where things are: for coming back after a long time
  const HB_LISTS = ['run', 'places', 'read_first', 'needs', 'terms'];
  const hbEmpty = h => !h || (!h.architecture && !h.installed && !HB_LISTS.some(k => (h[k] || []).length));
  // one line per command; lines break only between words, so `-Open` or `build-release.ps1` stay whole
  const cmdHtml = how => how.split('; ').map(line => line.split(' ').map(t => t.length <= 32 ? `<span class="nb">${esc(t)}</span>` : esc(t)).join(' ')).join('\n');
  function hbParts(h) {
    h = h || {};
    const kv = (items, key, val, keyClass) => items && items.length ? `<dl class="kv">${items.map(x => `<dt class="${keyClass}">${esc(x[key])}</dt><dd>${esc(x[val])}</dd>`).join('')}</dl>` : '';
    const i = h.installed;
    const inst = i ? `<div class="hb-inst">Installiert: <b>${esc(i.version)}</b> <button class="linkish mono faint" type="button" data-external="${esc(i.path)}" title="Im Explorer öffnen">${esc(i.path)}</button>${i.open ? `<div>Öffnen: ${esc(i.open)}</div>` : ''}</div>` : '';
    // a URL is opened, a command copied; a URL with a <placeholder> can only be copied and completed
    const act = r => /^https?:\/\/[^\s<]+$/.test(r.how)
      ? `<button class="linkish" type="button" data-external="${esc(r.how)}" aria-label="${esc(r.what)} öffnen">öffnen</button>`
      : `<button class="linkish" type="button" data-copy="${esc(r.how)}" aria-label="${esc(r.what)} kopieren">kopieren</button>`;
    return {
      // label on top, the command below at full width, one line per command; copying takes it as written
      run: inst + ((h.run || []).length ? `<div class="hb-runs">${h.run.map(r => `<div class="hb-run"><div class="w">${esc(r.what)}${r.folder && r.folder !== '.' ? `<span class="faint mono hb-in">in ${esc(r.folder)}</span>` : ''}</div><div class="cmd"><code class="mono">${cmdHtml(r.how)}</code>${act(r)}</div></div>`).join('')}</div>` : ''),
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
        ${goalLine(p)}${missing}${hbSec('So startest du es', x.run)}${hbSec('Zuerst lesen', x.read)}
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
    const statusBar = `<div class="statusbar"><span class="faint" style="font-size:13px">Deine Absicht:</span><span class="seg" role="group" aria-label="Status">${Object.entries(STATUS).map(([k, l]) => `<button type="button" data-status="${k}" aria-pressed="${p.status === k}">${l}</button>`).join('')}</span><input id="status-note" type="text" placeholder="Notiz an die Agents, z. B. warum geparkt oder worauf sie achten sollen" value="${esc(projectValue(p, 'status_note'))}" aria-label="Notiz an die Agents"></div>
      <div class="statusbar goalbar"><span class="faint" style="font-size:13px">Dein Ziel:</span><input id="goal-text" type="text" placeholder="z. B. v1.0 veröffentlichen, nur noch Bugfixes" value="${esc(projectValue(p, 'goal'))}" aria-label="Dein Ziel"><input id="goal-due" type="date" value="${esc(projectValue(p, 'goal_due'))}" aria-label="Bis wann" title="Bis wann (optional)"></div>`;
    const factsKv = f.is_git ? `<dl class="kv">
      <dt>Commits</dt><dd class="mono">${num(commits.total)} gesamt · 7 T: ${num(commits.last_7d)} · 30 T: ${num(commits.last_30d)} · seit ${commits.first ? fmtDate(commits.first) : '—'}</dd>
      <dt>Branch</dt><dd class="mono">${esc(f.branch || '—')}${f.upstream ? ` → ${esc(f.upstream)}` : ''}${f.ahead ? ` · ${f.ahead} voraus` : ''}${f.behind ? ` · ${f.behind} zurück` : ''}</dd>
      <dt>Remote</dt><dd>${f.has_remote ? esc(f.remotes.map(r => `${r.name}: ${r.url}`).join(', ')) : noRemoteTag(p)}</dd>
      <dt>Ungesichert</dt><dd>${s.unsaved ? `<span class="tag ${s.level}">${esc(s.unsaved)}</span>` : 'sauber'}</dd>
      <dt>Liegengeblieben</dt><dd>${esc(s.leftovers || 'nichts')}</dd>
      ${releaseText(p) ? `<dt>Release</dt><dd>${esc(releaseText(p))}</dd>` : ''}
      ${collisions(p).length ? `<dt>Überschneidungen</dt><dd>${collisions(p).map(c => `<div>${esc(collisionText(c))}</div>`).join('')}</dd>` : ''}
      ${f.night && (f.night.commits || f.night.uncommitted_touched) ? `<dt>Seit gestern Abend</dt><dd>${[f.night.commits ? `${plural(f.night.commits, 'Commit', 'Commits')}, +${num(f.night.lines_added)}/−${num(f.night.lines_removed)}` : '', f.night.reverts ? plural(f.night.reverts, 'Revert', 'Reverts') : '', f.night.uncommitted_touched ? `${plural(f.night.uncommitted_touched, 'Datei', 'Dateien')} ohne Commit bearbeitet` : ''].filter(Boolean).join(' · ')}</dd>` : ''}
      ${f.space && f.space.total_bytes ? `<dt>Platz</dt><dd>${bytes(f.space.total_bytes)} neu erzeugbar: ${f.space.dirs.filter(d => d.bytes >= 1e6).slice(0, 4).map(d => `${esc(d.path)} ${bytes(d.bytes)}`).join(' · ')}</dd>` : ''}
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
      const r = c.reentry;
      const hist = (p.history || []).filter((h, i, a) => i === a.length - 1 || h.headline !== a[i + 1].headline);
      const stampOf = at => new Date(at).toLocaleString('de-DE', { day: '2-digit', month: '2-digit', year: '2-digit', hour: '2-digit', minute: '2-digit' });
      const histRow = (h, i, a) => `<li><span class="when">${stampOf(h.at)}</span><span class="rail"></span><span class="what">${esc(h.headline)}${a[i + 1] && a[i + 1].phase !== h.phase ? ` <span class="pill ${esc(h.phase)}">${esc(PHASE[h.phase] || h.phase)}</span>` : ''}</span></li>`;
      const chronicle = hist.length > 1 ? `<ol class="timeline chron">${hist.slice(0, 8).map((h, i) => histRow(h, i, hist)).join('')}</ol>${hist.length > 8 ? `<details><summary class="more">+ ${hist.length - 8} ältere</summary><ol class="timeline chron">${hist.slice(8).map((h, i) => histRow(h, i + 8, hist)).join('')}</ol></details>` : ''}` : '';
      body += dmod('Wiedereinstieg', `<div class="reentry"><div class="t">${esc(r.next_step)}</div><div class="rmeta">${taskBtn(`reentry:${p.id}`, { projects: [p.id], title: r.next_step, why: r.why, done_when: r.done_when })}</div>${c.reentry.why ? `<div class="muted" style="font-size:14px">${esc(c.reentry.why)}</div>` : ''}</div>`)
        + dmod('Wartet auf dich', c.waiting_on_you.length ? `<ul class="qs">${c.waiting_on_you.map(w => question(w, true)).join('')}</ul>` : '')
        + dmod('Offene Enden', c.open_ends.length ? `<div class="list">${c.open_ends.map(o => `<div class="row"><div class="t">${esc(o.title)}</div><div style="display:flex;gap:6px;align-items:start"><span class="tag ${o.kind === 'uncommitted' || o.kind === 'ci' ? 'bad' : o.kind === 'unpushed' ? 'warn' : ''}">${esc(KIND[o.kind] || o.kind)}</span>${o.age_days != null ? `<span class="faint mono" style="font-size:12px">${o.age_days} T</span>` : ''}</div>${o.detail ? `<div class="d">${esc(o.detail)}</div>` : ''}${o.source ? `<div class="d mono" style="font-size:12px">${esc(o.source)}</div>` : ''}<div class="d">${taskBtn(`open:${p.id}:${o.title}`, { projects: [p.id], title: o.title, why: o.detail, evidence: o.source })}</div></div>`).join('')}</div>` : '', 'Angefangenes, Liegengebliebenes, Ungesichertes')
        + dmod('Zuletzt passiert', c.recent_changes.length ? `<div class="list">${[...c.recent_changes].sort((a, b) => b.date.localeCompare(a.date)).map(ch => `<div class="row"><div class="t">${esc(ch.title)}</div><div class="faint mono" style="font-size:12px">${fmtShort(ch.date)}</div>${ch.detail ? `<div class="d">${esc(ch.detail)}</div>` : ''}</div>`).join('')}</div>` : '')
        + dmod('Vorschläge', suggestions ? `<div class="cards2">${suggestions}</div>` : '')
        + dmod('Vision und Fortschritt', v.statement || v.progress_note ? `<div>${v.statement ? `<div>${esc(v.statement)}</div>` : ''}${v.progress_pct != null ? `<div class="prog"><span style="width:${Number(v.progress_pct)}%"></span></div><div class="faint mono" style="font-size:12px">${Number(v.progress_pct)} %</div>` : ''}${v.progress_note ? `<div class="muted" style="font-size:14px;margin-top:6px">${esc(v.progress_note)}</div>` : ''}</div>` : '')
        + dmod('Was es heute kann', c.features.length ? `<div class="feat">${c.features.map(x => `<span>${esc(x)}</span>`).join('')}</div>` : '')
        + dmod('Meilensteine', c.milestones.length ? `<ol class="timeline">${c.milestones.map(m => `<li><span class="when">${fmtDate(m.date)}</span><span class="rail"></span><span class="what">${esc(m.title)}</span></li>`).join('')}</ol>` : '')
        + dmod('Chronik', chronicle, 'Die Überschrift jeder Kartenversion, neueste zuerst')
        + dmod('Gesundheit', c.health_notes.length ? `<ul class="bullets">${c.health_notes.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : '')
        + dmod('Bewusst so entschieden', (c.decisions || []).length ? `<ul class="bullets">${c.decisions.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : '', 'Aus den Unterlagen des Projekts; die Agents werten das nicht als Risiko')
        + dmod('Querverbindungen', c.cross_project_hints.length ? `<ul class="bullets">${c.cross_project_hints.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : '')
        + (hbEmpty(c.handbook) ? '' : `<details class="mod hb" data-hb${hbOpen ? ' open' : ''}><summary class="mod-head"><div><h2>Handbuch <span class="chev" aria-hidden="true">▸</span></h2><div class="sub">Starten, Aufbau, wo was liegt: fürs Wiederreinkommen nach längerer Pause</div></div></summary>${handbookBody(c.handbook)}</details>`);
    }
    body += blockTasks(p.id) + warningControls(p) + dmod('Fakten aus Git', factsKv, p.facts_collected_at ? `gemessen vor ${minutesAgo(p.facts_collected_at)} Min${p.card_updated_at ? ` · Karte vom ${fmtDate(p.card_updated_at)}` : ''}` : null);
    return `<div class="scrim" data-close></div><aside class="drawer" role="dialog" aria-modal="true" aria-label="${esc(p.name)}">${body}</aside>`;
  }

  function openDrawer(id, keepScroll) {
    const p = byId(id);
    if (!p) return;
    const focused = keepScroll ? inputFocus() : null;
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
    else restoreInputFocus(focused);
  }
  const closeDrawer = () => { $('#drawer-root').innerHTML = ''; openId = null; };

  // ---------- decision round: every open question, one at a time ----------
  function startRound() {
    const queue = [...visible()].filter(p => p.card).sort((a, b) => risk(b) - risk(a)).flatMap(p => openQs(p.card).map(w => w.id)).filter(Boolean);
    if (!queue.length) return;
    closeDrawer();
    round = { queue, i: 0, stats: { answered: 0, done: 0, skipped: 0 }, busy: false };
    renderRound();
  }
  const closeRound = () => { round = null; $('#round-root').innerHTML = ''; };
  function renderRound(keepFocus = false) {
    const root = $('#round-root');
    if (!round) { root.innerHTML = ''; return; }
    if (round.busy) return;
    const previousInput = $('#round-answer');
    const hadFocus = previousInput && document.activeElement === previousInput;
    const selection = hadFocus ? [previousInput.selectionStart, previousInput.selectionEnd] : null;
    while (round.i < round.queue.length && (!findQ(round.queue[round.i]) || reply(round.queue[round.i]))) round.i++;
    const n = round.queue.length;
    let body;
    if (round.i >= n) {
      const st = round.stats;
      body = `<h3>Geschafft.</h3><p class="muted">${st.answered} beantwortet, ${st.done} erledigt, ${st.skipped} übersprungen. Die Agents lesen deine Antworten beim nächsten Lauf.</p>
        <div class="actions"><button class="btn primary" type="button" data-round-close>Schließen</button></div>`;
    } else {
      const { p, w } = findQ(round.queue[round.i]);
      const opts = (w.options || []).map((o, i) => `<button type="button" class="ropt" data-round-opt="${i}"><span class="k mono">${i + 1}</span><span><b>${esc(o.answer)}</b>${o.effect ? `<span class="muted">${esc(o.effect)}</span>` : ''}</span></button>`).join('');
      body = `<div class="rproj">${dot(p.id)}<b>${esc(p.name)}</b>${p.card.headline ? `<span class="faint"> · ${esc(p.card.headline)}</span>` : ''}</div>
        <h3>${esc(w.title)}</h3>${w.source ? `<div class="qsrc mono">${esc(w.source)}</div>` : ''}
        ${opts ? `<div class="ropts">${opts}</div>` : ''}
        <div class="inrow"><input type="text" id="round-answer" data-draft="${esc(w.id)}" data-kind="answer" value="${esc(drafts[w.id] || '')}" maxlength="600" placeholder="${opts ? 'Oder eigene Antwort' : 'Deine Antwort'}" aria-label="Deine Antwort"><button class="btn primary" type="button" data-round-send>Antworten</button></div>
        <div class="actions"><button class="btn" type="button" data-round-done title="Die Frage ist erledigt oder nicht mehr nötig">Erledigt</button><button class="btn" type="button" data-round-skip>Überspringen</button></div>`;
    }
    root.innerHTML = `<div class="scrim" data-round-close></div><div class="round" role="dialog" aria-modal="true" aria-label="Entscheidungsrunde">
      <div class="rhead"><span class="faint">Entscheidungsrunde${round.i < n ? ` · Frage ${round.i + 1} von ${n}` : ''}</span><button class="linkish" type="button" data-round-close>beenden</button></div>
      <div class="rprog"><span style="width:${(Math.min(round.i, n) / n * 100).toFixed(1)}%"></span></div>${body}</div>`;
    const input = $('#round-answer');
    if (!keepFocus || hadFocus) {
      if (input) {
        input.focus();
        if (keepFocus && selection) input.setSelectionRange(...selection);
      } else root.querySelector('[data-round-close].btn')?.focus();
    }
  }
  async function roundStep(value, note) {
    const active = round;
    if (!active || active.busy) return;
    const id = active.queue[active.i];
    if (!id || !findQ(id) || reply(id)) { renderRound(); return; }
    if (value === 'skip') { round.stats.skipped++; round.i++; renderRound(); return; }
    active.busy = true;
    document.querySelectorAll('#round-root button:not([data-round-close]), #round-answer').forEach(el => { el.disabled = true; });
    const saved = await react(id, value, note);
    // A save may finish after the owner closed this round or started another one.
    if (round !== active) return;
    active.busy = false;
    if (saved) { active.stats[value === 'answered' ? 'answered' : 'done']++; active.i++; delete drafts[id]; }
    renderRound();
  }

  // ---------- search ----------
  const norm = s => String(s).toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '');
  function searchEntries() {
    const out = [];
    for (const p of S.projects) {
      const add = (where, text, hb) => { if (text) out.push({ id: p.id, where, text: String(text), hb: !!hb }); };
      add('Name', p.name);
      add('Ordner', p.path);
      const c = p.card;
      if (!c) continue;
      add('Kurz gesagt', c.one_liner); add('Stack', c.stack); add('Überschrift', c.headline);
      if (c.vision && c.vision.statement) add('Vision', c.vision.statement);
      c.features.forEach(x => add('Kann', x));
      c.recent_changes.forEach(x => add('Zuletzt passiert', x.detail ? `${x.title} – ${x.detail}` : x.title));
      c.milestones.forEach(x => add('Meilenstein', x.title));
      c.open_ends.forEach(x => add('Offenes Ende', x.detail ? `${x.title} – ${x.detail}` : x.title));
      c.waiting_on_you.forEach(x => add('Frage an dich', x.title));
      c.suggestions.forEach(x => add('Vorschlag', `${x.title} – ${x.why}`));
      c.health_notes.forEach(x => add('Gesundheit', x));
      (c.decisions || []).forEach(x => add('Entscheidung', x));
      c.cross_project_hints.forEach(x => add('Querverbindung', x));
      const h = c.handbook || {};
      (h.run || []).forEach(x => add('Handbuch · Starten', `${x.what}: ${x.how}`, true));
      if (h.installed) add('Handbuch · Installiert', `${h.installed.version} ${h.installed.open || ''} ${h.installed.path}`, true);
      add('Handbuch · Aufbau', h.architecture, true);
      (h.places || []).forEach(x => add('Handbuch · Wo was liegt', `${x.path} – ${x.what}`, true));
      (h.read_first || []).forEach(x => add('Handbuch · Zuerst lesen', `${x.path} – ${x.why}`, true));
      (h.needs || []).forEach(x => add('Handbuch · Braucht', x, true));
      (h.terms || []).forEach(x => add('Handbuch · Begriff', `${x.term}: ${x.meaning}`, true));
    }
    return out;
  }
  function searchResults() {
    const words = norm(searchQ).split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    return searchEntries()
      .map(e => ({ e, hay: norm(`${byId(e.id).name} ${e.text}`) }))
      .filter(x => words.every(w => x.hay.includes(w)))
      .map(x => ({ ...x.e, score: (x.e.where === 'Name' ? 100 : 0) + words.filter(w => norm(x.e.text).includes(w)).length }))
      .sort((a, b) => b.score - a.score)
      .slice(0, 40);
  }
  function highlight(text) {
    // Match plain text first: replacing inside escaped HTML corrupts entities and
    // the <mark> tags inserted for earlier search words.
    const words = norm(searchQ).split(/\s+/).filter(Boolean).sort((a, b) => b.length - a.length);
    if (!words.length) return esc(text);
    const source = String(text), normalized = norm(source);
    const pattern = new RegExp(words.map(w => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|'), 'g');
    const offsets = [];
    for (let i = 0; i < source.length;) {
      const character = String.fromCodePoint(source.codePointAt(i));
      for (let n = 0; n < norm(character).length; n++) offsets.push(i);
      i += character.length;
    }
    offsets.push(source.length);
    let html = '', end = 0;
    for (const match of normalized.matchAll(pattern)) {
      const start = offsets[match.index], next = offsets[match.index + match[0].length];
      html += esc(source.slice(end, start)) + `<mark>${esc(source.slice(start, next))}</mark>`;
      end = next;
    }
    return html + esc(source.slice(end));
  }
  function renderSearchResults() {
    const box = $('#search-root .sresults');
    if (!box) return;
    const res = searchResults();
    searchSel = Math.min(searchSel, Math.max(0, res.length - 1));
    box.innerHTML = !searchQ.trim() ? '<p class="faint sempty">Sucht in allen Karten und Handbüchern: Funktionen, Befehle, Begriffe, Entscheidungen.</p>'
      : !res.length ? '<p class="faint sempty">Nichts gefunden.</p>'
      : res.map((r, i) => `<button class="sres" type="button" data-sres="${i}" data-id="${esc(r.id)}" data-hb="${r.hb}" aria-selected="${i === searchSel}"><span class="w">${dot(r.id)}<b>${esc(byId(r.id).name)}</b> · ${esc(r.where)}</span><span class="x">${highlight(r.text)}</span></button>`).join('');
    const sel = box.querySelector('[aria-selected="true"]');
    if (sel) sel.scrollIntoView({ block: 'nearest' });
  }
  function openSearch() {
    if (!S || !S.projects.length) return;
    searchOpen = true;
    $('#search-root').innerHTML = `<div class="scrim" data-search-close></div><div class="search" role="dialog" aria-modal="true" aria-label="Suche">
      <input type="search" id="search-q" placeholder="Suchen, z. B. Login, SQLite, Port" value="${esc(searchQ)}" aria-label="Suchbegriff" autocomplete="off">
      <div class="sresults"></div><div class="shint faint">↑ ↓ wählen · Enter öffnet den Steckbrief · Esc schließt</div></div>`;
    renderSearchResults();
    const input = $('#search-q');
    input.focus();
    input.select();
  }
  const closeSearch = () => { searchOpen = false; $('#search-root').innerHTML = ''; };
  function pickResult(btn) {
    if (!btn) return;
    if (btn.dataset.hb === 'true') hbOpen = true;
    closeSearch();
    openDrawer(btn.dataset.id);
    const hb = btn.dataset.hb === 'true' && document.querySelector('#drawer-root [data-hb]');
    if (hb) hb.scrollIntoView({ block: 'start' });
  }

  // ---------- page ----------
  function renderFoot() {
    const collapsed = [...collapsedMods()];
    const ratedCount = Object.keys(S.feedback).filter(id => rated(id) === 'down' || rated(id) === 'later').length;
    const archived = S.projects.filter(p => p.status === 'archived');
    const parts = [];
    if (collapsed.length) parts.push(`Eingeklappt: ${collapsed.map(id => `${esc(MOD_TITLES[id] || id)} <button type="button" data-mod-toggle="${esc(id)}">ausklappen</button>`).join(' · ')}`);
    if (ratedCount) parts.push(`${ratedCount} ${ratedCount === 1 ? 'bewerteter Vorschlag' : 'bewertete Vorschläge'} ${showRated ? (ratedCount === 1 ? 'wird angezeigt' : 'werden angezeigt') : 'ausgeblendet'} <button type="button" data-toggle-rated>${showRated ? 'wieder ausblenden' : 'anzeigen'}</button>`);
    if (archived.length) parts.push(`Archiviert: ${archived.map(p => plink(p.id)).join(', ')}`);
    parts.push(`Karten schreiben Agents mit <code class="mono">watchtower</code> · Fakten misst die App selbst`);
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
    return `<div class="empty"><div class="kicker">Willkommen</div><h2>Welche Projekte soll Watchtower im Blick behalten?</h2>
      <p class="muted">Wähle den Ordner, in dem deine Projekte liegen, oder direkt ein einzelnes Projekt. Watchtower zeigt dir die gefundenen Git-Repos zum Abhaken und misst sofort, wie es um sie steht. Die Einschätzungen schreibt danach ein Agent; wie du den einrichtest, steht in den Einstellungen.</p>
      ${discBusyOrListing('main') ? '' : '<div class="actions"><button class="btn primary" type="button" data-pick data-target="main">Ordner wählen …</button></div>'}
      ${discHtml('main')}</div>`;
  }

  // ---------- settings panel ----------
  function toggle(key, on, label, sub) {
    return `<div class="setting"><div><div class="t">${esc(label)}</div>${sub ? `<div class="d">${esc(sub)}</div>` : ''}</div><button class="toggle" type="button" role="switch" aria-checked="${on}" data-setting="${key}" aria-label="${esc(label)}"><span></span></button></div>`;
  }
  const copyLine = (text, label) => `<div class="copyline"><code class="mono">${esc(text)}</code><button class="btn" type="button" data-copy="${esc(text)}" aria-label="${esc(label)} kopieren">Kopieren</button></div>`;

  function projectEditor(p) {
    const edit = projectEdits[p.id];
    if (!edit) return '';
    const disabled = edit.busy ? ' disabled' : '';
    return `<div class="project-editor" aria-busy="${!!edit.busy}">
      <label for="project-name-${esc(p.id)}">Name in Watchtower</label>
      <input type="text" id="project-name-${esc(p.id)}" data-project-edit-field="name" data-project="${esc(p.id)}" value="${esc(edit.name)}"${disabled}>
      <label for="project-path-${esc(p.id)}">Projektordner</label>
      <div class="inrow"><input type="text" class="mono" id="project-path-${esc(p.id)}" data-project-edit-field="path" data-project="${esc(p.id)}" value="${esc(edit.path)}"${disabled}><button class="btn" type="button" data-project-folder="${esc(p.id)}"${disabled}>Ordner wählen …</button></div>
      <p class="d">Karten, Antworten und Verlauf bleiben erhalten. Wenn du den Ordner bereits verschoben oder umbenannt hast, wähle seinen neuen Ort. Watchtower verschiebt keine Dateien.</p>
      ${edit.error ? `<p class="project-edit-error bad-t" role="alert">${esc(edit.error)}</p>` : ''}
      <div class="actions"><button class="btn primary" type="button" data-project-save="${esc(p.id)}"${disabled}>${edit.busy === 'save' ? 'Speichert …' : 'Speichern'}</button><button class="btn" type="button" data-project-cancel="${esc(p.id)}"${disabled}>Abbrechen</button></div>
    </div>`;
  }

  function panelHtml() {
    const st = settings || {};
    const projects = S.projects.map(p => `<div class="prow"><div class="pmain"><div class="pname-s">${dot(p.id)}<b>${esc(p.name)}</b>${p.status !== 'active' ? `<span class="tag">${esc(STATUS[p.status])}</span>` : ''}</div><div class="d mono">${esc(p.path)}</div></div>
      ${projectEdits[p.id] ? '' : confirmRemove === p.id
        ? `<div class="confirm"><span>Nicht mehr beobachten? Karten, Antworten und Verlauf bleiben gespeichert.</span><button class="btn danger" type="button" data-remove="${esc(p.id)}">Entfernen</button><button class="btn" type="button" data-remove-cancel>Behalten</button></div>`
        : `<div class="project-actions"><button class="btn" type="button" data-project-edit="${esc(p.id)}">Bearbeiten</button><button class="linkish" type="button" data-remove-ask="${esc(p.id)}">entfernen</button></div>`}${projectEditor(p)}</div>`).join('');
    return `<div class="scrim" data-close-panel></div><aside class="drawer panel" role="dialog" aria-modal="true" aria-label="Einstellungen">
      <button class="close" data-close-panel type="button">Schließen</button>
      <h2 class="pname">Einstellungen</h2>
      <section class="mod"><div class="mod-head"><div><h2>Projekte</h2><div class="sub">${S.projects.length ? `${S.projects.length} im Blick. Status und Notiz setzt du im Steckbrief eines Projekts.` : 'Noch keine.'}</div></div></div>
        <div class="plist">${projects}</div>
        ${discBusyOrListing('panel') ? '' : '<div class="actions"><button class="btn" type="button" data-pick data-target="panel">Projekte hinzufügen …</button></div>'}
        ${discHtml('panel')}</section>
      <section class="mod"><div class="mod-head"><div><h2>Start</h2></div></div>
        ${toggle('autostart', !!st.autostart, 'Beim Anmelden öffnen', 'Watchtower geht auf, sobald du dich bei Windows anmeldest.')}</section>
      <section class="mod"><div class="mod-head"><div><h2>Agents</h2><div class="sub">Agents lesen deine Repos und schreiben die Einschätzungen mit dem Befehl „watchtower“.</div></div></div>
        ${toggle('agent_access', !!st.agent_access, 'Für Agents aufrufbar', 'Trägt den Watchtower-Ordner in deinen PATH ein. Programme, die schon laufen (etwa vBot), sehen das erst nach einem Neustart.')}
        <div class="setting-block"><div class="t">Skill für Agents</div><div class="d">Erklärt einem Agent den täglichen Lauf. Watchtower hält ihn bei jedem Start aktuell.</div>${copyLine(st.skill_dir || '', 'Pfad')}</div>
        <div class="setting-block"><div class="t">In vBot installieren</div><div class="d">Gib den Befehl dem Agent, der Watchtower betreuen soll. Er installiert den Skill nur für sich. Danach kannst du ihm den täglichen Lauf zuweisen.</div>${copyLine(st.vbot_install || '', 'Befehl')}</div></section>
      <section class="mod"><div class="mod-head"><div><h2>Über</h2></div></div>
        <dl class="kv"><dt>Version</dt><dd class="mono">${esc(st.version || '')}</dd><dt>Programm</dt><dd class="mono">${esc(st.exe || '')}</dd><dt>Daten</dt><dd class="mono">${esc(st.data_dir || '')}</dd></dl></section>
    </aside>`;
  }
  function renderPanel(keepScroll) {
    const root = $('#panel-root');
    if (!panelOpen) { root.innerHTML = ''; return; }
    const prev = root.querySelector('.drawer');
    const top = keepScroll && prev ? prev.scrollTop : 0;
    const focused = inputFocus();
    root.innerHTML = panelHtml();
    root.querySelector('.drawer').scrollTop = top;
    restoreInputFocus(focused);
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
    const focused = inputFocus();
    const d = new Date(S.today + 'T12:00:00');
    $('#date').textContent = d.toLocaleDateString('de-DE', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
    const collected = S.projects.map(p => p.facts_collected_at).filter(Boolean).sort()[0];
    $('#fresh').textContent = collected ? `Git-Fakten vor ${minutesAgo(collected)} Min` : '';
    renderRunStatus();
    $('#refresh').hidden = $('.switch').hidden = !S.projects.length;
    document.querySelectorAll('.switch button').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.style === S.prefs.style)));
    const style = { board: renderBoard, handbook: renderHandbook }[S.prefs.style] || renderHandover;
    $('#view').innerHTML = (S.projects.length ? blockChanges() : '') + blockTasks() + (!S.projects.length ? renderEmpty() : style());
    renderFoot();
    if (openId) openDrawer(openId, true);
    if (panelOpen) renderPanel(true);
    if (round) renderRound(true);
    restoreInputFocus(focused);
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
    catch (e) { $('#view').innerHTML = `<p class="note" style="margin-top:30px">Watchtower konnte seine Daten nicht laden: ${esc(errText(e))}</p>`; }
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
  async function react(id, value, note, msg, until) {
    const prev = S.feedback[id] ? { ...S.feedback[id] } : null;
    try {
      const result = await call('set_feedback', { item_id: id, value, note: note === undefined ? null : note, until: until === undefined ? null : until });
      const undoToken = typeof result.undo === 'string' ? result.undo.match(/^watchtower undo ([a-f0-9]{32})$/)?.[1] : null;
      if (value) S.feedback[id] = { value, at: new Date().toISOString(), note: typeof result.note === 'string' ? result.note : (note === undefined ? (prev ? prev.note : '') : note), snoozed_until: Object.hasOwn(result, 'snoozed_until') ? result.snoozed_until : (value === 'later' ? (until || (prev && prev.snoozed_until) || addDays(S.today, S.snooze_days || 7)) : null) }; else delete S.feedback[id];
      render();
      if (msg) toast(msg, undoToken ? async () => {
        noteFor = null;
        try {
          await call('undo', { token: undoToken });
          await load(false);
        } catch (e) {
          await load(false);
          toast('Rückgängig hat nicht geklappt: ' + errText(e));
        }
      } : null);
      return true;
    } catch (e) { toast('Nicht gespeichert: ' + errText(e)); return false; }
  }
  // the drawer renders after the page, so the last field is the one in front
  const focusDraft = id => { const all = document.querySelectorAll(`[data-draft="${CSS.escape(id)}"]`); if (all.length) all[all.length - 1].focus(); };
  async function setFeedback(el, v) {
    const id = el.dataset.item;
    if (v === 'later') { openSnooze(id); return; }
    const value = S.feedback[id] && S.feedback[id].value === v ? null : v;
    snoozeFor = null;
    noteFor = value === 'down' ? id : null;
    const msg = value === 'down' ? 'Ausgeblendet. Die Agents schlagen das nicht mehr vor.' : null;
    if (await react(id, value, undefined, msg)) focusDraft(id); else noteFor = null;
  }
  function openSnooze(id) {
    snoozeFor = id; noteFor = null;
    const f = S.feedback[id];
    snoozeDate = f && rated(id) === 'later' ? snoozedUntil(f) : addDays(S.today, 7);
    render();
  }
  async function saveSnooze(until) {
    const id = snoozeFor;
    if (!id) return;
    if (!/^\d{4}-\d{2}-\d{2}$/.test(until || '') || until <= S.today) { toast('Wähle einen Tag nach heute.'); return; }
    snoozeFor = null; noteFor = id;
    if (await react(id, 'later', undefined, `Wieder am ${fmtDate(until)}.`, until)) focusDraft(id);
    else { noteFor = null; snoozeFor = id; render(); }
  }
  async function assignTask(key) {
    const snapshot = TASKS[key];
    if (!snapshot || Number.isInteger(snapshot.id) || busyTasks.has(key)) return;
    busyTasks.add(key);
    try {
      const result = await call('create_task', { payload: snapshot });
      await load(false);
      toast(result.created ? `Auftrag #${result.task.id} ist als beauftragt gemerkt. Kopiere ihn für deinen Agent.` : `Auftrag #${result.task.id} ist bereits beauftragt.`);
    } catch (e) { toast('Auftrag nicht gespeichert: ' + errText(e)); }
    finally { busyTasks.delete(key); }
  }
  async function changeTaskStatus(id, status, revision) {
    const key = `tracked:${id}`;
    if (busyTasks.has(key)) return;
    busyTasks.add(key);
    try {
      await call('set_task_status', { id, status, revision }); await load(false);
      toast(status === 'done' ? 'Ergebnis bestätigt.' : status === 'assigned' ? 'Auftrag wieder freigegeben. Kopiere ihn für deinen Agent.' : 'Auftrag beendet.');
    } catch (e) {
      if (e && e.code === 'task_changed') { await load(false); toast('Der Auftrag hat sich inzwischen geändert. Prüfe zuerst das neue Ergebnis.'); }
      else toast('Auftrag nicht geändert: ' + errText(e));
    }
    finally { busyTasks.delete(key); }
  }
  async function saveWarning(project, key, clear = false) {
    const draftKey = `${project}:${key}`, note = clear ? null : (warningDrafts[draftKey] || '').trim();
    if (!clear && !note) { toast('Schreib kurz, warum das bewusst so ist.'); return; }
    try {
      await call('set_warning_ack', { project, key, note });
      warningFor = null; delete warningDrafts[draftKey]; await load(false);
      toast(clear ? 'Dieser Befund wird wieder als Warnung gezeigt.' : 'Als bewusste Entscheidung gemerkt. Geänderte Befunde werden wieder gemeldet.');
    } catch (e) { toast('Nicht gespeichert: ' + errText(e)); }
  }
  async function markChangesRead(token) {
    try { await call('mark_changes_read', { token }); await load(false); toast('Lesestand gemerkt. Neu hinzugekommene Änderungen bleiben sichtbar.'); }
    catch (e) { toast('Lesestand nicht gespeichert: ' + errText(e)); }
  }
  async function copyTask(key) {
    const task = TASKS[key];
    if (!task) return;
    try { await navigator.clipboard.writeText(taskPrompt(task)); toast('Auftrag kopiert. Füg ihn bei deinem Coding-Agent ein.'); }
    catch (e) { toast('Kopieren hat nicht geklappt.'); }
  }
  async function saveNote(id) {
    const text = (drafts[id] || '').trim();
    noteFor = null;
    if (!text || !S.feedback[id]) { delete drafts[id]; render(); return; }
    if (await react(id, S.feedback[id].value, text)) {
      delete drafts[id];
      toast('Gemerkt. Die Agents lesen das beim nächsten Lauf.');
    } else { noteFor = id; render(); focusDraft(id); }
  }
  async function saveAnswer(id) {
    const text = (drafts[id] || '').trim();
    if (!text) { focusDraft(id); return; }
    answering = null;
    if (await react(id, 'answered', text, 'Beantwortet. Der nächste Lauf nimmt deine Antwort in die Karte auf.')) delete drafts[id];
    else { answering = id; render(); focusDraft(id); }
  }
  async function addIdea() {
    const el = $('#idea-text'), text = el ? el.value.trim() : '';
    if (!text) { if (el) el.focus(); return; }
    try {
      await call('add_idea', { text });
      if (ideaDraft.trim() === text) ideaDraft = '';
      await load(false);
      toast('Idee geparkt. Der nächste Agent-Lauf schätzt sie ein.');
    } catch (e) { toast(`Nicht gespeichert: ${errText(e)}`); }
  }
  async function removeIdea(id) {
    try {
      await call('remove_idea', { id });
      await load(false);
      toast('Idee entfernt. Sie bleibt gespeichert.', async () => {
        try {
          await call('restore_idea', { id });
          await load(false);
          toast('Idee wiederhergestellt.');
        } catch (e) { toast(`Nicht wiederhergestellt: ${errText(e)}`); }
      });
    } catch (e) { toast(`Nicht entfernt: ${errText(e)}`); }
  }
  async function setPref(key, value) {
    const previous = S.prefs[key];
    S.prefs[key] = value;
    render();
    try { await call('set_pref', { key, value }); }
    catch (e) {
      if (S.prefs[key] === value) { S.prefs[key] = previous; render(); }
      toast('Einstellung nicht gespeichert.');
    }
  }
  async function toggleModule(id) {
    const collapsed = collapsedMods();
    if (collapsed.has(id)) collapsed.delete(id); else collapsed.add(id);
    await setPref('hidden_modules', [...collapsed]);
    $(`[data-mod-toggle="${CSS.escape(id)}"]`)?.focus({ preventScroll: true });
  }
  async function setStatus(status, note) {
    const id = openId;
    try { await call('set_status', { id, status, note }); clearProjectDraft(id, 'status_note', note); await load(false); }
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
  function editProject(id) {
    const p = byId(id);
    if (!p) return;
    projectEdits[id] ||= { name: p.name, path: p.path, originalName: p.name, originalPath: p.path, busy: null, error: null };
    confirmRemove = null;
    renderPanel(true);
    $(`#project-name-${CSS.escape(id)}`).focus();
  }
  function cancelProjectEdit(id) {
    if (!projectEdits[id] || projectEdits[id].busy) return;
    delete projectEdits[id];
    renderPanel(true);
  }
  async function pickProjectFolder(id) {
    const edit = projectEdits[id];
    if (!edit || edit.busy) return;
    edit.busy = 'pick'; edit.error = null;
    renderPanel(true);
    try {
      const path = await call('pick_folder');
      if (path) edit.path = path;
    } catch (e) { edit.error = `Ordner konnte nicht gewählt werden: ${errText(e)}`; }
    finally { edit.busy = null; renderPanel(true); }
  }
  async function saveProjectEdit(id) {
    const edit = projectEdits[id];
    if (!edit || edit.busy) return;
    const name = edit.name.trim(), path = edit.path.trim();
    if (!name || !path) {
      edit.error = !name ? 'Gib einen Namen ein.' : 'Gib den vorhandenen Projektordner an.';
      renderPanel(true); return;
    }
    const args = { id };
    if (name !== edit.originalName) args.name = name;
    if (path !== edit.originalPath) args.path = path;
    if (!args.name && !args.path) { cancelProjectEdit(id); return; }
    edit.busy = 'save'; edit.error = null;
    renderPanel(true);
    try {
      const result = await call('update_project', args);
      delete projectEdits[id];
      await load(false);
      toast(result.facts_refresh && result.facts_refresh.ok === false
        ? 'Projekt geändert. Fakten konnten noch nicht neu gemessen werden. Versuche es mit „Aktualisieren“ erneut.'
        : 'Projekt geändert. Karten und Verlauf bleiben erhalten.');
    } catch (e) {
      edit.busy = null;
      edit.error = `Nicht gespeichert: ${errText(e)}`;
      renderPanel(true);
    }
  }
  async function removeProject(id) {
    const p = byId(id);
    try {
      await call('remove_project', { id });
      confirmRemove = null;
      await load(false);
      toast(`${p ? p.name : id} ist nicht mehr im Blick. Karten und Verlauf bleiben gespeichert.`, async () => {
        try {
          await call('restore_project', { id });
          await load(false);
          toast(`${p ? p.name : id} ist wieder im Blick.`);
        } catch (e) { toast(`Nicht wiederhergestellt: ${errText(e)}`); }
      });
    } catch (e) { toast(errText(e)); }
  }

  document.addEventListener('click', async e => {
    const t = e.target;
    if ($('#report-status').open && !t.closest('#report-status')) $('#report-status').open = false;
    const expand = t.closest('[data-expand]');
    if (expand && expand.dataset.expand === 'ideas') { expanded.ideas = !expanded.ideas; render(); $('[data-expand="ideas"]').focus({ preventScroll: true }); return; }
    const snoozeOpen = t.closest('[data-snooze-open]');
    if (snoozeOpen) { openSnooze(snoozeOpen.dataset.snoozeOpen); return; }
    const snoozeDays = t.closest('[data-snooze-days]');
    if (snoozeDays) { saveSnooze(addDays(S.today, Number(snoozeDays.dataset.snoozeDays))); return; }
    if (t.closest('[data-snooze-save]')) { saveSnooze(snoozeDate); return; }
    if (t.closest('[data-snooze-cancel]')) { snoozeFor = null; render(); return; }
    const snoozeClear = t.closest('[data-snooze-clear]');
    if (snoozeClear) { snoozeFor = null; noteFor = null; react(snoozeClear.dataset.snoozeClear, null, null, 'Wird wieder angezeigt.'); return; }
    const assign = t.closest('[data-task-assign]');
    if (assign) { assignTask(assign.dataset.taskAssign); return; }
    const taskStatus = t.closest('[data-task-status]');
    if (taskStatus) { changeTaskStatus(Number(taskStatus.dataset.taskId), taskStatus.dataset.taskStatus, Number(taskStatus.dataset.taskRevision)); return; }
    const warningOpen = t.closest('[data-warning-open]');
    if (warningOpen) { warningFor = `${warningOpen.dataset.project}:${warningOpen.dataset.warningOpen}`; render(); return; }
    const warningSave = t.closest('[data-warning-save]');
    if (warningSave) { saveWarning(warningSave.dataset.project, warningSave.dataset.warningSave); return; }
    const warningClear = t.closest('[data-warning-clear]');
    if (warningClear) { saveWarning(warningClear.dataset.project, warningClear.dataset.warningClear, true); return; }
    if (t.closest('[data-warning-cancel]')) { warningFor = null; render(); return; }
    const read = t.closest('[data-changes-read]');
    if (read) { markChangesRead(read.dataset.changesRead); return; }
    if (t.closest('[data-round-start]')) { startRound(); return; }
    if (t.closest('[data-round-close]')) { closeRound(); return; }
    if (round) {
      const ro = t.closest('[data-round-opt]');
      if (ro) { const q = findQ(round.queue[round.i]); const option = q && (q.w.options || [])[Number(ro.dataset.roundOpt)]; if (option) roundStep('answered', option.answer); else renderRound(); return; }
      if (t.closest('[data-round-send]')) { const v = ($('#round-answer').value || '').trim(); if (v) roundStep('answered', v); else $('#round-answer').focus(); return; }
      if (t.closest('[data-round-done]')) { roundStep('done', null); return; }
      if (t.closest('[data-round-skip]')) { roundStep('skip'); return; }
    }
    const opt = t.closest('[data-opt]');
    if (opt) { const q = findQ(opt.dataset.opt); if (q) react(q.w.id, 'answered', q.w.options[Number(opt.dataset.i)].answer, 'Beantwortet. Der nächste Lauf nimmt deine Antwort in die Karte auf.'); return; }
    if (t.closest('[data-search]')) { openSearch(); return; }
    if (t.closest('[data-search-close]')) { closeSearch(); return; }
    const sr = t.closest('[data-sres]');
    if (sr) { pickResult(sr); return; }
    if (t.closest('[data-idea-add]')) { addIdea(); return; }
    const ir = t.closest('[data-idea-remove]');
    if (ir) { removeIdea(Number(ir.dataset.ideaRemove)); return; }
    const tk = t.closest('[data-task]');
    if (tk) { copyTask(tk.dataset.task); return; }
    const sw = t.closest('.switch button');
    if (sw) { setPref('style', sw.dataset.style); window.scrollTo(0, 0); return; }
    const fbBtn = t.closest('.fb button');
    if (fbBtn) { e.stopPropagation(); setFeedback(fbBtn.parentElement, fbBtn.dataset.v); return; }
    const moduleToggle = t.closest('[data-mod-toggle]');
    if (moduleToggle) { toggleModule(moduleToggle.dataset.modToggle); return; }
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
    const pe = t.closest('[data-project-edit]');
    if (pe) { editProject(pe.dataset.projectEdit); return; }
    const pf = t.closest('[data-project-folder]');
    if (pf) { pickProjectFolder(pf.dataset.projectFolder); return; }
    const ps = t.closest('[data-project-save]');
    if (ps) { saveProjectEdit(ps.dataset.projectSave); return; }
    const pc = t.closest('[data-project-cancel]');
    if (pc) { cancelProjectEdit(pc.dataset.projectCancel); return; }
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
    if ((e.target.id === 'goal-text' || e.target.id === 'goal-due') && openId) {
      const id = openId, p = byId(id), goal = $('#goal-text').value, due = $('#goal-due').value || null;
      try {
        await call('set_goal', { id, goal, due });
        p.goal = goal.trim(); p.goal_due = p.goal ? due : null;
        clearProjectDraft(id, 'goal', goal); clearProjectDraft(id, 'goal_due', due || '');
        toast(p.goal ? 'Ziel gespeichert. Die Agents messen den Stand ab dem nächsten Lauf daran.' : 'Ziel entfernt.');
      } catch (err) { toast('Ziel nicht gespeichert: ' + errText(err)); }
      return;
    }
    if (e.target.id === 'status-note' && openId) {
      const id = openId, p = byId(id), note = e.target.value;
      try { await call('set_status', { id, status: p.status, note }); p.status_note = note; clearProjectDraft(id, 'status_note', note); toast('Notiz gespeichert.'); }
      catch (err) { toast('Notiz nicht gespeichert.'); }
    }
  });
  // toggle does not bubble, so listen while it travels down
  document.addEventListener('toggle', e => { if (e.target.matches && e.target.matches('[data-hb]')) hbOpen = e.target.open; }, true);
  document.addEventListener('input', e => { if (e.target.id === 'search-q') { searchQ = e.target.value; searchSel = 0; renderSearchResults(); } });
  document.addEventListener('input', e => { if (e.target.matches('[data-draft]')) drafts[e.target.dataset.draft] = e.target.value; });
  document.addEventListener('input', e => {
    if (e.target.id === 'idea-text') ideaDraft = e.target.value;
    if (e.target.id === 'snooze-date') snoozeDate = e.target.value;
    if (e.target.dataset && e.target.dataset.warningNote) warningDrafts[e.target.dataset.warningNote] = e.target.value;
    const editField = e.target.dataset && e.target.dataset.projectEditField;
    const edit = editField && projectEdits[e.target.dataset.project];
    if (edit && !edit.busy && (editField === 'name' || editField === 'path')) edit[editField] = e.target.value;
    const field = { 'status-note': 'status_note', 'goal-text': 'goal', 'goal-due': 'goal_due' }[e.target.id];
    if (field && openId) (projectDrafts[openId] ||= {})[field] = e.target.value;
  });
  document.addEventListener('keydown', e => {
    if (e.target.id === 'idea-text' && e.key === 'Enter') { e.preventDefault(); addIdea(); return; }
    if (round) {
      if (e.key === 'Escape') { e.preventDefault(); closeRound(); return; }
      const inInput = e.target.id === 'round-answer';
      if (inInput && e.key === 'Enter') { e.preventDefault(); const v = e.target.value.trim(); if (v) roundStep('answered', v); return; }
      if (!inInput) {
        const k = Number(e.key), q = findQ(round.queue[round.i] || '');
        if (k >= 1 && q && q.w.options && q.w.options[k - 1]) { e.preventDefault(); roundStep('answered', q.w.options[k - 1].answer); }
      }
      return;
    }
    if (searchOpen) {
      const n = $('#search-root').querySelectorAll('.sres').length;
      if (e.key === 'Escape') { e.preventDefault(); closeSearch(); }
      else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); if (n) { searchSel = (searchSel + (e.key === 'ArrowDown' ? 1 : n - 1)) % n; renderSearchResults(); } }
      else if (e.key === 'Enter') { e.preventDefault(); pickResult($('#search-root .sres[aria-selected="true"]')); }
      return;
    }
    if ((e.key === 'k' && (e.ctrlKey || e.metaKey)) || (e.key === '/' && !typing())) { e.preventDefault(); openSearch(); return; }
    if (e.target.dataset && e.target.dataset.projectEditField && (e.key === 'Enter' || e.key === 'Escape')) {
      e.preventDefault();
      if (e.key === 'Enter') saveProjectEdit(e.target.dataset.project);
      else cancelProjectEdit(e.target.dataset.project);
      return;
    }
    const draft = e.target.closest && e.target.closest('[data-draft]');
    if (draft && (e.key === 'Enter' || e.key === 'Escape')) {
      e.preventDefault();
      const id = draft.dataset.draft, note = draft.dataset.kind === 'note';
      if (e.key === 'Enter') { if (note) saveNote(id); else saveAnswer(id); }
      else { if (note) { delete drafts[id]; noteFor = null; } else answering = null; render(); }
      return;
    }
    if (e.key === 'Escape') {
      if ($('#report-status').open) { e.preventDefault(); $('#report-status').open = false; $('#report-status-summary').focus(); }
      else if (panelOpen) closePanel();
      else closeDrawer();
    }
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
