import { S, call, errText, load, render, setPref, toast } from './app.js';
import { fbButtons, hiddenByRating, myNote, noteFor, noteForm, openQs, question, questionAge, reply } from './feedback.js';
import { taskBtn, trackedTask } from './handoff.js';
import { ideaCard } from './ideas.js';
import { $, CI, STATUS, VERDICT, VERDICT_STATUS, ago, byId, byLastWork, bytes, collapsedMods, days, daysSince, dmod, dot, esc, fmtDate, fmtShort, foldAttrs, lastWork, mod, num, openSince, plink, shownRow, streak, streakText, visible } from './presentation.js';
import { cardFreshness, goalLine, openProjectTarget, phasePill, projectUnread } from './projects.js';

let showAllRecent = false;

let warningFor = null;
const warningDrafts = {};

// ---------- facts-derived safety ----------
const acknowledged = (p, key) => !!(p.warnings || []).find(w => w.key === key && w.acknowledged);
function noRemoteTag(p) {
  return acknowledged(p, 'no_remote') ? '<span class="tag">bewusst ohne Remote</span>' : '<span class="tag">kein Remote eingerichtet</span>';
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
  const out = { remote: f.has_remote, unsaved: null, level: '', ahead: Number.isFinite(f.ahead) ? f.ahead : null, ci: (f.ci || {}).state || 'unknown', leftovers: null };
  if (d.files) {
    // the saved observations know how long uncommitted work has existed; file times reset with every commit
    const run = streak(p, 'uncommitted');
    const age = run ? run.days : d.oldest_change ? daysSince(d.oldest_change) : 0;
    const since = run && run.days > 0 ? `, ${streakText(run)}` : age > 0 ? `, seit ${days(age)}` : '';
    const lines = d.line_counts?.ok === true && Number.isFinite(d.lines_added) && Number.isFinite(d.lines_removed)
      ? `+${num(d.lines_added)}/−${num(d.lines_removed)}` : 'Zeilenzahl unbekannt (nicht vollständig gemessen)';
    out.unsaved = `${num(d.files)} Dateien, ${lines}${since}`;
    out.unsavedShort = `${num(d.files)} ${d.files === 1 ? 'Datei' : 'Dateien'} offen${age > 0 ? ` · ${age} T${run && run.trend === 'growing' ? ' ↑' : ''}` : ''}`;
  }
  const unpushed = streak(p, 'unpushed');
  out.aheadSince = unpushed && unpushed.days > 0 ? streakText(unpushed) : '';
  const ciRun = f.ci && f.ci.at && out.ci === 'red' ? daysSince(f.ci.at) : null;
  out.ciAge = ciRun != null && ciRun >= 2 ? `letzter Lauf vor ${days(ciRun)}` : '';
  const unmerged = ((f.branches || {}).unmerged || []).length;
  const wts = (f.worktrees || {}).count || 0;
  const parts = [];
  if (unmerged) parts.push(`${unmerged} ungemergte ${unmerged === 1 ? 'Branch' : 'Branches'}`);
  if (wts) parts.push(`${wts} ${wts === 1 ? 'Worktree' : 'Worktrees'}${(f.worktrees || {}).prunable ? `, ${(f.worktrees || {}).prunable} tot` : ''}`);
  out.leftovers = parts.join(' · ') || null;
  return out;
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
    const labels = { no_card: 'Einschätzung fehlt', changed: 'Projekt hat sich geändert', ci_changed: 'CI hat sich geändert', answered: 'Deine Antwort fehlt noch im Bericht', status_changed: 'Deine Vorgaben haben sich geändert', new_fields: 'Einschätzung muss ergänzt werden', stale: 'Einschätzung ist veraltet', idea_review: 'Ideen werden neu durchdacht', missing: 'Projektüberblick fehlt', new_day: 'Projektüberblick für heute fehlt', owner_idea: 'Deine neue Idee muss eingeschätzt werden', cards_changed: 'Projektberichte wurden aktualisiert', cards_will_change: 'Offene Projektberichte müssen zuerst aktualisiert werden', facts_unreadable: 'Fakten konnten nicht gelesen werden', facts_missing: 'Fakten fehlen', path_missing: 'Projektordner fehlt', not_a_git_repository: 'Kein Git-Repository', git_status_failed: 'Git-Status konnte nicht gelesen werden', agent_failed: 'Agent konnte die Prüfung nicht abschließen', project_removed: 'Projekt nicht mehr beobachtet', project_archived: 'Projekt archiviert' };
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
  $('#report-status-body').innerHTML = `<p class="faint">${esc(timing)}</p>${r.state !== 'complete' && started ? `<p>Aktuelle Prüfung begonnen: ${esc(started)}.</p>` : ''}${outcomes}${portfolioText ? `<p>${esc(portfolioText)}</p>` : ''}${lines ? `<ul class="report-status-problems">${lines}</ul>` : ''}${pending ? `<p>${pending} ${pending === 1 ? 'Antwort wartet' : 'Antworten warten'} noch darauf, in die Projektberichte übernommen zu werden.</p>` : ''}${r.state === 'unknown' ? '<p>Ein Agent prüft die Projekte und schreibt bei Bedarf neue Berichte. „Git-Fakten messen“ erneuert keine Agent-Berichte.</p>' : ''}${repair ? `<button class="btn" type="button" data-copy="${esc(repair)}">Auftrag zum Fortsetzen kopieren</button>` : ''}`;
}

function blockChanges() {
  const c = S.changes;
  if (!c) return '';
  const entries = c.entries || [];
  const unread = visible().filter(projectUnread);
  if (c.has_baseline && !entries.length && !unread.length && !collapsedMods().has('changes')) return `<p class="digest-empty faint">Keine neuen Projektmeldungen seit deinem Lesestand${c.since ? ` vom ${fmtDate(c.since)}` : ''}.</p>`;
  const row = e => `<div class="change-item"><div class="h"><span class="change-kind">${esc(({ new: 'Neu', resolved: 'Nicht mehr offen', decision_applied: 'Antwort übernommen' })[e.kind] || e.kind)}</span>${e.project ? plink(e.project) : ''}</div><div class="t">${esc(e.title)}</div>${e.detail ? `<div class="why">${esc(e.detail)}</div>` : ''}${e.next_step ? `<div class="why"><b>Nächster Schritt:</b> ${esc(e.next_step)}</div>` : ''}</div>`;
  const groups = new Map();
  for (const e of entries) { const key = e.project || ''; if (!groups.has(key)) groups.set(key, []); groups.get(key).push(e); }
  const body = entries.length ? `<div class="digest-groups">${[...groups].map(([id, items]) => `<details class="digest-project" ${foldAttrs(`digest:${id}`)}><summary><b>${esc((byId(id) || {}).name || 'Projektüberblick')}</b><span class="count">${items.length}</span><span class="digest-preview">${esc(items[0].title)}</span></summary><div class="digest-items">${items.map(row).join('')}</div></details>`).join('')}</div>` : `<p class="faint">${c.has_baseline ? 'Projektberichte enthalten ungelesene Änderungen, etwa im Handbuch oder in der Projektstimme.' : 'Markiere den aktuellen Stand ausdrücklich als gelesen. Ein Besuch allein setzt deinen Lesestand nicht.'}</p>`;
  const also = unread.filter(p => !groups.has(p.id));
  const other = also.length ? `<p class="digest-other">Weitere ungelesene Berichte: ${also.map(p => plink(p.id)).join(' · ')}</p>` : '';
  const action = c.token && (entries.length || unread.length || !c.has_baseline) ? `<div class="actions"><button class="btn" type="button" data-changes-read="${esc(c.token)}">${c.has_baseline ? 'Diese Änderungen als gelesen markieren' : 'Aktuellen Stand als gelesen markieren'}</button></div>` : '';
  return mod('changes', 'Seit deinem letzten Lesen', `Projektmeldungen${c.since ? ` · Lesestand vom ${fmtDate(c.since)}` : ''}`, `<div class="digest-summary">${body}${other}${action}</div>`);
}


function blockFocus() {
  const t = (S.portfolio && S.portfolio.focus) || [];
  if (!t.length) return '';
  const date = S.portfolio_updated_at ? ` · Einschätzung vom ${fmtDate(S.portfolio_updated_at)}` : '';
  const carried = it => { const row = shownRow((S.brief_persistence || {}).items, r => r.id === it.id && r.kind === 'focus'); return row && row.versions >= 3 && row.days >= 3 ? `<span class="faint q-age">seit ${days(row.days)} im Fokus</span>` : ''; };
  return mod('focus', 'Als Nächstes', `Vom Agent über alle Projekte hinweg ausgewählt${date}`, `<div class="focus">${t.slice(0, 3).map((it, i) => `
    <div class="item${reply(it.id) ? ' done' : ''}"><div class="n">${i + 1}</div><div><div class="t">${esc(it.title)} ${carried(it)}</div>
    <div class="meta">${plink(it.project)}${taskBtn(it.id || `focus-${i}`, { projects: [it.project], title: it.title, why: it.why, done_when: it.done_when })}${it.id ? `<button type="button" class="check" data-focus-done="${esc(it.id)}" aria-pressed="${!!reply(it.id)}">${reply(it.id) ? '✓ erledigt' : 'erledigt?'}</button>` : ''}</div>
    <div class="why">${esc(it.why)}</div>${byId(it.project) && byId(it.project).card_outdated ? '<p class="report-freshness">Projekt seit der Einschätzung verändert · vor dem Auftrag prüfen</p>' : ''}</div></div>`).join('')}</div>`);
}

// what the facts say about a project, as a few short badges

const plural = (n, one, many) => `${num(n)} ${n === 1 ? one : many}`;
// two lines of work that change the same files may conflict when merged
const lineName = l => l.kind === 'main' ? 'lokale Änderungen im Hauptordner' : l.kind === 'worktree' ? `Worktree ${l.name}` : `Branch ${l.name}`;
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
  if (s.remote === false) out.push(noRemoteTag(p));
  if (s.unsavedShort) out.push(`<span class="tag ${s.level}">${esc(s.unsavedShort)}</span>`);
  if (s.ahead > 0) out.push(`<span class="tag"${s.aheadSince ? ` title="${esc(s.aheadSince)}"` : ''}>${s.ahead} lokale Commits vor Upstream</span>`);
  if (s.ahead == null) out.push('<span class="tag">Push-Stand unbekannt</span>');
  if (s.ci === 'red') out.push(`<span class="tag bad"${s.ciAge ? ` title="${esc(s.ciAge)}"` : ''}>CI rot</span>`);
  if (!s.unsaved && f.dirty && f.dirty.files === 0) out.push('<span class="tag">Arbeitsstand sauber</span>');
  out.push(collisionTag(p));
  return out.join('');
}
// Rank owner action above routine repository activity. Counts of dirty files,
// local commits and worktrees are facts, not a measure of urgency.
const projectTasks = p => (S.tasks || []).filter(t => (t.projects || []).includes(p.id));
const goalIsDue = p => !!(p.goal && p.goal_due && p.goal_due <= S.today && p.status !== 'done');
const risk = p => (projectTasks(p).some(t => t.status === 'blocked') ? 100 : 0)
  + (projectTasks(p).some(t => t.status === 'review') ? 80 : 0)
  + (goalIsDue(p) ? 60 : 0)
  + (p.card ? Math.min(openQs(p.card).length, 2) * 20 : 0)
  + (safety(p).ci === 'red' ? 8 : 0)
  + (collisions(p).length && !acknowledged(p, 'worktrees') ? 5 : 0)
  + (projectUnread(p) ? 2 : 0);
const byAttention = list => [...list].sort((a, b) => risk(b) - risk(a) || (lastWork(b) || '').localeCompare(lastWork(a) || '') || a.name.localeCompare(b.name));

function blockProjects() {
  const list = byAttention(visible());
  if (!list.length) return '';
  const tile = p => {
    const c = p.card, waiting = c ? openQs(c).length : 0;
    const fresh = projectUnread(p) ? '<span class="pill new">ungelesen</span>' : '';
    const tasks = projectTasks(p), blocked = tasks.filter(t => t.status === 'blocked').length, review = tasks.filter(t => t.status === 'review').length;
    return `<button class="tile" type="button" data-open="${esc(p.id)}">
      <div class="h"><span class="name">${dot(p.id)}${esc(p.name)}</span><span class="pills">${fresh}${p.status !== 'active' ? `<span class="pill unknown">${esc(STATUS[p.status])}</span>` : phasePill(p)}</span></div>
      <p class="voice">${c ? `„${esc(c.personality || c.headline)}“` : '<span class="faint">Noch keine Einschätzung. Kommt beim nächsten Agent-Lauf.</span>'}</p>
      ${c && c.personality ? `<p class="project-headline">${esc(c.headline)}</p>` : ''}
      ${goalLine(p)}
      <div class="badges">${blocked ? `<span class="tag ask">${blocked} blockiert</span>` : ''}${review ? `<span class="tag ask">${review} zur Prüfung</span>` : ''}${waiting ? `<span class="tag ask">${waiting} ${waiting === 1 ? 'Entscheidung' : 'Entscheidungen'}</span>` : ''}${safety(p).ci === 'red' ? '<span class="tag bad">CI rot</span>' : ''}</div>
      <div class="tfoot">${cardFreshness(p)}</div>
    </button>`;
  };
  return mod('projects', 'Deine Projekte', 'Entscheidungen, wartende Ergebnisse und fällige Ziele zuerst', `<div class="tiles">${list.map(tile).join('')}</div>`);
}

// one question: answer it or mark it done; answered ones show the reply until the next card

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
  const items = visible().flatMap(p => (p.card ? p.card.recent_changes : []).map((c, index) => ({ ...c, project: p.id, reportedAt: p.card_updated_at || '', unread: (p.unread_recent_changes || []).includes(index) }))).filter(c => daysSince(c.date) <= 14).sort((a, b) => b.date.localeCompare(a.date) || b.reportedAt.localeCompare(a.reportedAt));
  if (!items.length) return '';
  const shownItems = showAllRecent ? items : items.slice(0, limit);
  let last = '';
  const body = shownItems.map(c => {
    const h = c.date !== last ? `<div class="day-h">${fmtShort(c.date)} · ${ago(c.date)}</div>` : '';
    last = c.date;
    const fresh = c.unread ? ' <span class="pill new">ungelesen</span>' : '';
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
    const unsaved = [s.unsaved ? `<span>${esc(s.unsaved)}</span>` : p.facts.dirty && p.facts.dirty.files === 0 ? 'Arbeitsstand sauber' : 'Arbeitsstand unbekannt', s.ahead == null ? 'Push-Stand unbekannt' : s.ahead > 0 ? `${s.ahead} lokale Commits vor Upstream${s.aheadSince ? `, ${esc(s.aheadSince)}` : ''}` : 'Kein Vorsprung zum lokalen Upstream-Stand'].join('<br>');
    const ci = CI[s.ci] || CI.unknown;
    return `<tr><td>${plink(p.id)}</td>
      <td>${s.remote ? `<span class="tag">${esc((p.facts.remotes || []).map(r => r.name).join(', ') || 'Remote eingerichtet')}</span>` : s.remote === false ? noRemoteTag(p) : '<span class="faint">unbekannt</span>'}</td>
      <td>${unsaved}</td>
      <td>${s.leftovers ? `<span class="faint">${esc(s.leftovers)}</span>` : ''} ${collisionTag(p)}</td>
      <td>${ci[1] ? `<span class="tag ${ci[1]}">${ci[0]}</span>` : `<span class="faint">${ci[0]}</span>`}${s.ciAge ? `<div class="faint">${esc(s.ciAge)}</div>` : ''}</td></tr>`;
  }).join('');
  if (!rows) return '';
  return mod('safety', 'Git-Zustand', 'Lokal gemessene Fakten. Remotes belegen keine Sicherung; der Upstream-Stand wird ohne Fetch gelesen.', `<div class="tablewrap"><table class="safety"><thead><tr><th>Projekt</th><th>Remote</th><th>Arbeitsstand</th><th>Weitere Arbeitslinien</th><th>CI</th></tr></thead><tbody>${rows}</tbody></table></div>`);
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
  const list = visible().filter(p => p.facts?.space?.ok !== true || p.facts.space.total_bytes >= 1e8).sort((a, b) => (b.facts?.space?.total_bytes || 0) - (a.facts?.space?.total_bytes || 0));
  const complete = list.every(p => p.facts?.space?.ok === true && Number.isFinite(p.facts.space.total_bytes));
  const total = complete ? list.reduce((a, p) => a + p.facts.space.total_bytes, 0) : null;
  if (complete && total < 1e9) return '';
  const rows = list.map(p => {
    const sp = p.facts?.space || {}, phase = p.card && p.card.phase;
    const resting = phase === 'dormant' || phase === 'done' || p.status === 'parked' || p.status === 'done';
    const dirs = (sp.dirs || []).filter(d => d.ok !== true || d.bytes >= 1e6).slice(0, 4).map(d => `<button class="linkish mono" type="button" data-external="${esc(p.path + '\\' + d.path.replace(/\//g, '\\'))}" title="Im Explorer öffnen">${esc(d.path)}</button> ${d.ok === true && Number.isFinite(d.bytes) ? bytes(d.bytes) : 'Größe unbekannt'}`).join(' · ');
    return `<div class="row${resting ? ' resting' : ''}"><div class="t">${plink(p.id)}${resting ? ' <span class="tag good">ruht</span>' : ''}</div><div class="mono">${sp.ok === true && Number.isFinite(sp.total_bytes) ? bytes(sp.total_bytes) : 'Größe unbekannt'}</div><div class="d">${dirs}</div></div>`;
  }).join('');
  return mod('space', 'Platz', complete ? `${bytes(total)} in Build- und Paketordnern, die sich neu erzeugen lassen. Einmal am Tag gemessen` : 'Gesamtgröße unbekannt: Nicht alle Ordner konnten vollständig gemessen werden.', `<div class="list">${rows}</div>`);
}

// patterns, synergies and verdicts: what shows only when all projects lie side by side
function blockAcross() {
  const pf = S.portfolio || {};
  const verdict = r => {
    const v = VERDICT[r.verdict] || [r.verdict, ''];
    const name = byId(r.project) ? byId(r.project).name : r.project;
    if (noteFor === r.id) return noteForm(r.id, `${v[0]}: ${name}`);
    if (hiddenByRating(r.id)) return '';
    const p = byId(r.project), target = VERDICT_STATUS[r.verdict];
    const adopt = p && target && p.status !== target ? `<div class="taskrow"><span class="task-actions"><button class="linkish task" type="button" data-verdict-status="${esc(target)}" data-project="${esc(p.id)}" title="Setzt deine Absicht für das Projekt; die Agents richten sich danach">Als „${esc(STATUS[target])}“ übernehmen</button></span></div>` : '';
    return `<div class="idea"><div class="kind">Parken oder zurückholen?</div><div class="h"><div class="t">${esc(name)}</div><span class="tag ${v[1]}">${esc(v[0])}</span></div>
      <div class="why">${esc(r.why)}</div>${myNote(r.id)}<div class="foot"><div class="who">${plink(r.project)}</div>${fbButtons(r.id)}</div>${adopt}</div>`;
  };
  const cards = [
    ...(pf.patterns || []).map(s => ideaCard(s, s.projects, null, 'Muster')),
    ...(pf.synergies || []).map(s => ideaCard(s, s.projects, null, 'Querverbindung')),
    ...(pf.park_or_revive || []).map(verdict),
  ].filter(Boolean);
  return cards.length ? mod('across', 'Quer durch alle Projekte', 'Was man erst sieht, wenn alle nebeneinander liegen', `<div class="cards3">${cards.join('')}</div>`) : '';
}


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

// ---------- Today: what needs the owner's attention ----------
let showAllAttention = false;
let inspectedTask = null;
function attentionItems() {
  const projects = byAttention(visible());
  const tasks = (S.tasks || []).filter(t => ['review', 'blocked'].includes(t.status)).map(t => ({ type: 'task', task: t, score: t.status === 'blocked' ? 100 : 90 }));
  const goals = projects.filter(goalIsDue).map(p => ({ type: 'goal', p, score: 80, title: p.goal }));
  const questions = projects.flatMap(p => p.card ? openQs(p.card).map((q, i) => ({ type: 'question', p, q, title: q.title, score: 50 - i, age: openSince(questionAge(q.id)) })) : []);
  const problems = projects.flatMap(p => {
    if (p.facts?.error === 'path_missing') return [{ type: 'problem', p, score: 75, title: 'Projektordner fehlt – Pfad prüfen', label: 'Ordner fehlt' }];
    const s = safety(p);
    // a risk that keeps growing for a week is news even when nobody asks about it
    const growing = ['uncommitted', 'unpushed'].map(kind => streak(p, kind)).filter(r => r && r.trend === 'growing' && r.days >= 7 && p.status === 'active')
      .map(r => ({ type: 'problem', p, score: 65, label: 'Wächst', title: r.kind === 'uncommitted' ? `${num(r.now.files)} Dateien nicht committet, vorher ${num(r.then.files)}` : `${num(r.now.commits)} Commits nicht gepusht, vorher ${num(r.then.commits)}`, age: streakText({ ...r, trend: null }) }));
    return [...growing, ...(s.ci === 'red' ? [{ type: 'problem', p, score: 70, title: 'CI meldet fehlgeschlagene Prüfungen', label: 'CI rot', age: s.ciAge }] : [])];
  });
  return [...tasks, ...goals, ...problems, ...questions].sort((a, b) => b.score - a.score);
}
function blockAttention() {
  const items = attentionItems(), questions = items.filter(i => i.type === 'question').length;
  const selected = inspectedTask && (S.tasks || []).find(t => t.id === inspectedTask);
  const inspection = selected ? `<div class="task-inspection"><button type="button" class="linkish" data-task-inspection-close>Auftragsansicht schließen</button>${trackedTask(selected, true)}</div>` : '';
  if (!items.length) return `<section class="morning-attention quiet"><h2>Gerade wartet nichts auf dich</h2><p class="muted">Keine offenen Entscheidungen, blockierten Aufträge oder Ergebnisse zur Bestätigung.</p>${inspection}</section>`;
  const row = item => {
    if (item.type === 'task') return `<details class="attention-row" ${foldAttrs(`attention:${item.task.id}`)}><summary><span class="tag ask">${item.task.status === 'blocked' ? 'Blockiert' : 'Ergebnis prüfen'}</span><span class="attention-title">${esc(item.task.title)}</span><span class="faint">#${item.task.id}</span></summary><div class="attention-detail">${trackedTask(item.task)}</div></details>`;
    const label = item.label || (item.type === 'goal' ? (item.p.goal_due < S.today ? 'Ziel überfällig' : 'Ziel heute fällig') : 'Entscheidung');
    return `<div class="attention-row"><span class="tag ask">${label}</span><button type="button" class="linkish attention-title" data-attention-project="${esc(item.p.id)}" data-attention-tab="${item.type === 'problem' ? 'history' : 'work'}" data-attention-anchor="${esc(item.q ? `question:${item.q.id}` : item.type === 'problem' ? 'facts' : 'owner')}">${esc(item.title)}</button><span class="attention-project">${esc(item.p.name)}${item.age ? ` · ${esc(item.age)}` : ''}</span></div>`;
  };
  return `<section class="morning-attention"><div class="attention-head"><div><h2>Braucht dich</h2><p class="muted">${items.length} ${items.length === 1 ? 'Punkt wartet' : 'Punkte warten'} auf eine Entscheidung oder Prüfung.</p></div>${questions ? `<button class="btn" type="button" data-round-start>${questions === 1 ? 'Entscheidung beantworten' : `${questions} Entscheidungen nacheinander`}</button>` : ''}</div><div class="attention-list">${(showAllAttention ? items : items.slice(0, 5)).map(row).join('')}</div>${items.length > 5 ? `<button class="more" type="button" data-attention-all>${showAllAttention ? 'wieder weniger anzeigen' : `alle ${items.length} anzeigen`}</button>` : ''}${inspection}</section>`;
}
async function openTaskResult(id) {
  if (!(S.tasks || []).some(t => t.id === id)) return;
  inspectedTask = id;
  await setPref('style', 'handover');
  render();
  const el = document.querySelector(`.task-inspection [data-search-anchor="task:${id}"]`);
  if (el) { el.classList?.add('search-hit'); el.scrollIntoView?.({ block: 'center' }); el.focus({ preventScroll: true }); }
}
function renderHandover() {
  const n = visible().length;
  const greeting = (S.portfolio && S.portfolio.greeting) || `Guten Morgen. ${n} ${n === 1 ? 'Projekt' : 'Projekte'} im Blick.`;
  return `<div class="handover">
    <div class="greet"><div class="kicker">Heute · ${n} ${n === 1 ? 'Projekt' : 'Projekte'}</div><h2>${esc(greeting)}</h2></div>
    ${blockAttention()}${blockFocus()}${blockChanges()}
    ${missingCardsNote()}
  </div>`;
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

function handleAttentionClick(e, t) {
  const attention = t.closest('[data-attention-project]');
  if (attention) { openProjectTarget(attention.dataset.attentionProject, attention.dataset.attentionTab || 'work', attention.dataset.attentionAnchor); return true; }
  if (t.closest('[data-attention-all]')) { showAllAttention = !showAllAttention; render(); return true; }
  if (t.closest('[data-task-inspection-close]')) { inspectedTask = null; render(); return true; }
  return false;
}

function handleOverviewClick(e, t) {
  const warningOpen = t.closest('[data-warning-open]');
  if (warningOpen) { warningFor = `${warningOpen.dataset.project}:${warningOpen.dataset.warningOpen}`; render(); return true; }
  const warningSave = t.closest('[data-warning-save]');
  if (warningSave) { saveWarning(warningSave.dataset.project, warningSave.dataset.warningSave); return true; }
  const warningClear = t.closest('[data-warning-clear]');
  if (warningClear) { saveWarning(warningClear.dataset.project, warningClear.dataset.warningClear, true); return true; }
  if (t.closest('[data-warning-cancel]')) { warningFor = null; render(); return true; }
  if (t.closest('[data-recent-all]')) { showAllRecent = !showAllRecent; render(); return true; }
  return false;
}

function handleWarningInput(e) {
  if (e.target.dataset && e.target.dataset.warningNote) warningDrafts[e.target.dataset.warningNote] = e.target.value;
  return false;
}

export { blockAcross, blockAttention, blockNight, blockProjects, blockRecent, blockSafety, blockSpace, blockWeek, byAttention, collisionText, collisions, handleAttentionClick, handleOverviewClick, handleWarningInput, markChangesRead, noRemoteTag, openTaskResult, plural, projectTasks, releaseText, renderHandover, renderRunStatus, risk, safety, warningControls };
