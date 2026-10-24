import { tr, locale } from './i18n.js';
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
  return acknowledged(p, 'no_remote') ? `<span class="tag">${tr("intentionally without a remote")}</span>` : `<span class="tag">${tr("no remote configured")}</span>`;
}
function warningControls(p) {
  const warnings = p.warnings || [];
  if (!warnings.length) return '';
  const row = w => {
    const key = `${p.id}:${w.key}`;
    const control = w.can_acknowledge === false ? '' : w.acknowledged
      ? `<span>${esc(w.note)}</span><button class="linkish" type="button" data-warning-clear="${esc(w.key)}" data-project="${esc(p.id)}">${tr("warn again")}</button>`
      : warningFor === key ? `<div class="inrow"><input type="text" data-warning-note="${esc(key)}" aria-label="${tr("Why is this intentional?")}" maxlength="300" value="${esc(warningDrafts[key] || '')}" placeholder="${tr("e.g. intentionally local, no GitHub Project")}"><button class="btn" type="button" data-warning-save="${esc(w.key)}" data-project="${esc(p.id)}">${tr("Save")}</button><button class="linkish" type="button" data-warning-cancel>${tr("Cancel")}</button></div>`
        : `<button class="linkish" type="button" data-warning-open="${esc(w.key)}" data-project="${esc(p.id)}">${tr("Intentional")}</button>`;
    return `<div class="warning-row"><strong>${esc(w.key === 'ci_red' ? tr('CI failed\u0004warning') : tr(w.title || w.kind || w.key))}</strong>${control}</div>`;
  };
  const active = warnings.filter(w => !w.acknowledged), known = warnings.filter(w => w.acknowledged);
  return dmod(tr("Review warnings"), active.map(row).join('') + (known.length ? `<details class="acknowledged-warnings"><summary>${tr("{0} intentionally accepted", known.length)}</summary>${known.map(row).join('')}</details>` : ''), tr("Your reason applies to this finding. vWatch warns again if it changes materially."));
}
function safety(p) {
  const f = p.facts || {};
  const d = f.dirty || {};
  const out = { remote: f.has_remote, unsaved: null, level: '', ahead: Number.isFinite(f.ahead) ? f.ahead : null, ci: (f.ci || {}).state || 'unknown', leftovers: null };
  if (d.files) {
    // the saved observations know how long uncommitted work has existed; file times reset with every commit
    const run = streak(p, 'uncommitted');
    const age = run ? run.days : d.oldest_change ? daysSince(d.oldest_change) : 0;
    const since = run && run.days > 0 ? `, ${streakText(run)}` : age > 0 ? tr(", for {0}", days(age)) : '';
    const lines = d.line_counts?.ok === true && Number.isFinite(d.lines_added) && Number.isFinite(d.lines_removed)
      ? `+${num(d.lines_added)}/−${num(d.lines_removed)}` : tr("Line count unknown (measurement incomplete)");
    out.unsaved = tr("{0} files, {1}{2}", num(d.files), lines, since);
    out.unsavedShort = tr("{0} {1} uncommitted{2}", num(d.files), d.files === 1 ? tr("file") : tr("files"), age > 0 ? ` ${tr("· {0} d{1}", age, run && run.trend === 'growing' ? ' ↑' : '')}` : '');
  }
  const unpushed = streak(p, 'unpushed');
  out.aheadSince = unpushed && unpushed.days > 0 ? streakText(unpushed) : '';
  const ciRun = f.ci && f.ci.at && out.ci === 'red' ? daysSince(f.ci.at) : null;
  out.ciAge = ciRun != null && ciRun >= 2 ? tr("last Run {0} ago", days(ciRun)) : '';
  const unmerged = ((f.branches || {}).unmerged || []).length;
  const wts = (f.worktrees || {}).count || 0;
  const parts = [];
  if (unmerged) parts.push(tr("{0} unmerged {1}", unmerged, unmerged === 1 ? 'Branch' : 'Branches'));
  if (wts) parts.push(`${wts} ${wts === 1 ? 'Worktree' : 'Worktrees'}${(f.worktrees || {}).prunable ? tr(", {0} stale", (f.worktrees || {}).prunable) : ''}`);
  out.leftovers = parts.join(' · ') || null;
  return out;
}


function renderRunStatus() {
  const r = S.run_status;
  const control = $('#report-status');
  control.hidden = !r;
  if (!r) return;
  const labels = { unknown: tr("Reports not checked yet"), running: tr("Checking reports"), stalled: tr("Review incomplete"), incomplete: tr("Review incomplete"), failed: tr("Review failed"), complete: tr("Reports checked") };
  const latest = r.latest || {}, counts = latest.counts || {};
  const pending = Array.isArray(r.pending_owner_replies) ? r.pending_owner_replies.length : Number(r.pending_owner_replies || 0);
  const reasonText = value => {
    if (Array.isArray(value)) return value.map(reasonText).join(', ');
    if (value && typeof value === 'object') return reasonText(value.message || value.code);
    const labels = { no_card: tr("Assessment missing"), changed: tr("Project changed"), ci_changed: tr("CI changed"), answered: tr("Your answer is not in the report yet"), status_changed: tr("Your instructions changed"), new_fields: tr("Assessment needs additional fields"), stale: tr("Assessment is outdated"), idea_review: tr("Ideas are being reconsidered"), missing: tr("Portfolio brief missing"), new_day: tr("Today's portfolio brief missing"), owner_idea: tr("Your new idea needs assessment"), cards_changed: tr("Project reports updated"), cards_will_change: tr("Pending Project reports need updating first"), facts_unreadable: tr("Facts could not be read"), facts_missing: tr("Facts missing"), path_missing: tr("Project folder missing"), not_a_git_repository: tr("Not a Git repository"), git_status_failed: tr("Git status could not be read"), agent_failed: tr("Agent could not complete the review"), project_removed: tr("Project no longer watched"), project_archived: tr("Project archived") };
    return labels[value] || (typeof value === 'string' && !/^[a-z]+(?:_[a-z]+)+$/.test(value) ? value : tr("Further review needed"));
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
  const outcomes = group(tr("Reports updated"), updated) + group(tr("Retained unchanged"), unchanged)
    + (unspecified.length ? `${group(tr("Reports checked"), unspecified)}<p class="faint">${tr("Whether these reports were revised was not recorded.")}</p>` : '')
    + (!checked.length && counts.current ? `<p>${tr("{0} of {1} Project reports were current. Individual updates were not recorded.", counts.current, counts.total || counts.current)}</p>` : '');
  const problems = projects.filter(p => !completed(p) && !excluded(p));
  const lines = problems.map(p => `<li>${name(p)}: ${esc(({ pending: tr("pending"), failed: tr("failed"), changed: tr("changed again"), running: tr("in progress") })[p.status] || tr("still open"))}${p.error || p.reason ? ` · ${esc(reasonText(p.error || p.reason))}` : ''}</li>`).join('');
  const repair = typeof r.repair_task === 'string' ? r.repair_task : r.repair_task && (r.repair_task.prompt || r.repair_task.text);
  const pf = latest.portfolio;
  const portfolioText = !pf ? '' : completed(pf)
    ? versionChange(pf) > 0 || pf.status === 'written' ? tr("Portfolio brief rewritten. It brings together priorities, connections and ideas.")
      : versionChange(pf) === 0 ? tr("Portfolio brief reviewed and retained unchanged.")
        : tr("Portfolio brief reviewed. Whether it was revised was not recorded.")
    : tr("Portfolio brief {0}{1}", pf.status === 'failed' ? tr("could not be updated") : tr("pending"), pf.error || pf.reason ? `: ${reasonText(pf.error || pf.reason)}` : '.');
  const timestamp = value => {
    const date = new Date(value);
    return value && Number.isFinite(date.getTime()) ? tr("{0}", date.toLocaleString(locale(), { day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit' })) : null;
  };
  const finished = timestamp(r.last_successful && r.last_successful.finished_at || (r.state === 'complete' && latest.finished_at));
  const started = timestamp(latest.started_at);
  const timing = finished ? tr("Last complete review: {0}.", finished) : r.state === 'complete' ? tr("Review complete; completion time not recorded.") : tr("No complete review recorded yet.");
  // Keep the native details element itself in place so refresh preserves open state and summary focus.
  control.setAttribute('data-state', pending && r.state === 'complete' ? 'incomplete' : r.state || 'unknown');
  $('#report-status-summary').textContent = `${labels[r.state] || labels.unknown}${pending ? ` · ${pending} ${pending === 1 ? tr("answer pending") : tr("answers pending")}` : ''}`;
  $('#report-status-body').innerHTML = `<p class="faint">${esc(timing)}</p>${r.state !== 'complete' && started ? `<p>${tr("Current review started: {0}.", esc(started))}</p>` : ''}${outcomes}${portfolioText ? `<p>${esc(portfolioText)}</p>` : ''}${lines ? `<ul class="report-status-problems">${lines}</ul>` : ''}${pending ? `<p>${tr("{0} {1} to be incorporated into the Project reports.", pending, pending === 1 ? tr("answer is waiting") : tr("answers are waiting"))}</p>` : ''}${r.state === 'unknown' ? `<p>${tr("An Agent reviews Projects and writes reports as needed. “Measure Git facts” does not update Agent reports.")}</p>` : ''}${repair ? `<button class="btn" type="button" data-copy="${esc(repair)}">${tr("Copy Task to resume")}</button>` : ''}`;
}

const digestTitle = e => e.title_prefix ? `${tr(e.title_prefix)}: ${e.source_title}` : e.title;
function blockChanges() {
  const c = S.changes;
  if (!c) return '';
  const entries = c.entries || [];
  const unread = visible().filter(projectUnread);
  if (c.has_baseline && !entries.length && !unread.length && !collapsedMods().has('changes')) return `<p class="digest-empty faint">${tr("No new Project updates since your reading baseline{0}.", c.since ? ` ${tr("from {0}", fmtDate(c.since))}` : '')}</p>`;
  const row = e => `<div class="change-item"><div class="h"><span class="change-kind">${esc(({ new: tr("New"), resolved: tr("No longer open"), decision_applied: tr("Answer incorporated") })[e.kind] || e.kind)}</span>${e.project ? plink(e.project) : ''}</div><div class="t">${esc(digestTitle(e))}</div>${e.detail ? `<div class="why">${esc(e.generated_detail ? tr(e.detail) : e.detail)}</div>` : ''}${e.next_step ? `<div class="why"><b>${tr("Next step:")}</b> ${esc(tr(e.next_step))}</div>` : ''}</div>`;
  const groups = new Map();
  for (const e of entries) { const key = e.project || ''; if (!groups.has(key)) groups.set(key, []); groups.get(key).push(e); }
  const body = entries.length ? `<div class="digest-groups">${[...groups].map(([id, items]) => `<details class="digest-project" ${foldAttrs(`digest:${id}`)}><summary><b>${esc((byId(id) || {}).name || tr("Portfolio brief"))}</b><span class="count">${items.length}</span><span class="digest-preview">${esc(digestTitle(items[0]))}</span></summary><div class="digest-items">${items.map(row).join('')}</div></details>`).join('')}</div>` : `<p class="faint">${c.has_baseline ? tr("Project reports contain unread changes, for example in the handbook or Project voice.") : tr("Explicitly mark the current state as read. Opening the app alone does not set your reading baseline.")}</p>`;
  const also = unread.filter(p => !groups.has(p.id));
  const other = also.length ? `<p class="digest-other">${tr("More unread reports: {0}", also.map(p => plink(p.id)).join(' · '))}</p>` : '';
  const action = c.token && (entries.length || unread.length || !c.has_baseline) ? `<div class="actions"><button class="btn" type="button" data-changes-read="${esc(c.token)}">${c.has_baseline ? tr("Mark these changes as read") : tr("Mark current state as read")}</button></div>` : '';
  return mod('changes', tr("Since your last read"), tr("Project updates{0}", c.since ? ` ${tr("· reading baseline from {0}", fmtDate(c.since))}` : ''), `<div class="digest-summary">${body}${other}${action}</div>`);
}


function blockFocus() {
  const t = (S.portfolio && S.portfolio.focus) || [];
  if (!t.length) return '';
  const date = S.portfolio_updated_at ? ` ${tr("· assessment from {0}", fmtDate(S.portfolio_updated_at))}` : '';
  const carried = it => { const row = shownRow((S.brief_persistence || {}).items, r => r.id === it.id && r.kind === 'focus'); return row && row.versions >= 3 && row.days >= 3 ? `<span class="faint q-age">${tr("in focus for {0}", days(row.days))}</span>` : ''; };
  return mod('focus', tr("Up next"), tr("Selected by the Agent across all Projects{0}", date), `<div class="focus">${t.slice(0, 3).map((it, i) => `
    <div class="item${reply(it.id) ? ' done' : ''}"><div class="n">${i + 1}</div><div><div class="t">${esc(it.title)} ${carried(it)}</div>
    <div class="meta">${plink(it.project)}${taskBtn(it.id || `focus-${i}`, { projects: [it.project], title: it.title, why: it.why, done_when: it.done_when })}${it.id ? `<button type="button" class="check" data-focus-done="${esc(it.id)}" aria-pressed="${!!reply(it.id)}">${reply(it.id) ? tr("✓ done") : tr("done?")}</button>` : ''}</div>
    <div class="why">${esc(it.why)}</div>${byId(it.project) && byId(it.project).card_outdated ? `<p class="report-freshness">${tr("Project changed since assessment · check before assigning")}</p>` : ''}</div></div>`).join('')}</div>`);
}

// what the facts say about a project, as a few short badges

const plural = (n, one, many) => `${num(n)} ${n === 1 ? one : many}`;
// two lines of work that change the same files may conflict when merged
const lineName = l => l.kind === 'main' ? tr("local changes in the main folder") : l.kind === 'worktree' ? `Worktree ${l.name}` : `Branch ${l.name}`;
const collisions = p => (p.facts && p.facts.collisions) || [];
const collisionText = c => tr("{0} and {1} change {2}: {3}{4}", lineName(c.a), lineName(c.b), plural(c.count, tr("the same file"), tr("the same files")), c.files.join(', '), c.count > c.files.length ? ' …' : '');
function collisionTag(p) {
  const list = collisions(p);
  return list.length ? `<span class="tag warn" title="${esc(list.map(collisionText).join('\n'))}">${list.length === 1 ? tr("Overlap") : tr("{0} overlaps", list.length)}</span>` : '';
}
const bareVersion = v => String(v || '').trim().replace(/^v/i, '');
// the newest tag, the work since, and whether the installed copy is older
function releaseText(p) {
  const r = p.facts && p.facts.release;
  if (!r) return null;
  const installed = p.card && p.card.handbook && p.card.handbook.installed;
  const inst = installed && installed.version && bareVersion(installed.version) !== bareVersion(r.tag) ? ` ${tr("· installed: {0}", installed.version)}` : '';
  return `${r.tag}${r.date ? ` ${tr("from {0}", fmtDate(r.date))}` : ''} · ${r.commits_since ? tr("{0} since then", plural(r.commits_since, 'Commit', 'Commits')) : tr("none since then")}${inst}`;
}
const unreleasedTag = p => { const r = p.facts && p.facts.release; return r && r.commits_since >= 10 ? `<span class="tag" title="${esc(releaseText(p))}">${tr("{0} since {1}", num(r.commits_since), esc(r.tag))}</span>` : ''; };

function signals(p) {
  const f = p.facts || {};
  if (!f.is_git) return `<span class="tag">${f.error === 'path_missing' ? tr("Folder missing") : tr("not Git")}</span>`;
  const s = safety(p);
  const out = [];
  if (s.remote === false) out.push(noRemoteTag(p));
  if (s.unsavedShort) out.push(`<span class="tag ${s.level}">${esc(s.unsavedShort)}</span>`);
  if (s.ahead > 0) out.push(`<span class="tag"${s.aheadSince ? ` title="${esc(s.aheadSince)}"` : ''}>${tr("{0} local commits ahead of upstream", s.ahead)}</span>`);
  if (s.ahead == null) out.push(`<span class="tag">${tr("Push state unknown")}</span>`);
  if (s.ci === 'red') out.push(`<span class="tag bad"${s.ciAge ? ` title="${esc(s.ciAge)}"` : ''}>${tr("CI failed")}</span>`);
  if (!s.unsaved && f.dirty && f.dirty.files === 0) out.push(`<span class="tag">${tr("Working tree clean")}</span>`);
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
    const fresh = projectUnread(p) ? `<span class="pill new">${tr("unread")}</span>` : '';
    const tasks = projectTasks(p), blocked = tasks.filter(t => t.status === 'blocked').length, review = tasks.filter(t => t.status === 'review').length;
    return `<button class="tile" type="button" data-open="${esc(p.id)}">
      <div class="h"><span class="name">${dot(p.id)}${esc(p.name)}</span><span class="pills">${fresh}${p.status !== 'active' ? `<span class="pill unknown">${esc(STATUS[p.status])}</span>` : phasePill(p)}</span></div>
      <p class="voice">${c ? `„${esc(c.personality || c.headline)}“` : `<span class="faint">${tr("No assessment yet. The next Agent Run will write it.")}</span>`}</p>
      ${c && c.personality ? `<p class="project-headline">${esc(c.headline)}</p>` : ''}
      ${goalLine(p)}
      <div class="badges">${blocked ? `<span class="tag ask">${tr("{0} blocked", blocked)}</span>` : ''}${review ? `<span class="tag ask">${tr("{0} for review", review)}</span>` : ''}${waiting ? `<span class="tag ask">${waiting} ${waiting === 1 ? tr("Decision") : tr("Decisions")}</span>` : ''}${safety(p).ci === 'red' ? `<span class="tag bad">${tr("CI failed")}</span>` : ''}</div>
      <div class="tfoot">${cardFreshness(p)}</div>
    </button>`;
  };
  return mod('projects', tr("Your Projects"), tr("Decisions, pending results and due goals first"), `<div class="tiles">${list.map(tile).join('')}</div>`);
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
      <ul class="qs">${head.map(w => question(w)).join('')}</ul>${rest.length ? `<details><summary class="more">${tr("+ {0} more", rest.length)}</summary><ul class="qs">${rest.map(w => question(w)).join('')}</ul></details>` : ''}</div>`;
  }).join('');
  const start = total > 1 ? `<div class="round-start"><button class="btn primary" type="button" data-round-start>${tr("Decision round: all {0} in sequence", total)}</button></div>` : '';
  return mod('waiting', tr("Waiting for you"), tr("{0} {1} only you can make. Agents read your answers in the next Run", total, total === 1 ? tr("Decision") : tr("Decisions")), `${start}<div class="groups">${body}</div>`);
}

function blockRecent(limit = 10) {
  const items = visible().flatMap(p => (p.card ? p.card.recent_changes : []).map((c, index) => ({ ...c, project: p.id, reportedAt: p.card_updated_at || '', unread: (p.unread_recent_changes || []).includes(index) }))).filter(c => daysSince(c.date) <= 14).sort((a, b) => b.date.localeCompare(a.date) || b.reportedAt.localeCompare(a.reportedAt));
  if (!items.length) return '';
  const shownItems = showAllRecent ? items : items.slice(0, limit);
  let last = '';
  const body = shownItems.map(c => {
    const h = c.date !== last ? `<div class="day-h">${fmtShort(c.date)} · ${ago(c.date)}</div>` : '';
    last = c.date;
    const fresh = c.unread ? ` <span class="pill new">${tr("unread")}</span>` : '';
    const p = byId(c.project);
    const head = `<span class="t">${esc(c.title)}${fresh}</span><span class="pmark">${dot(c.project)}${esc(p ? p.name : c.project)}</span>`;
    return `${h}${c.detail ? `<details class="ev-item"><summary>${head}</summary><p>${esc(c.detail)}</p></details>` : `<div class="ev-item"><div class="sum">${head}</div></div>`}`;
  }).join('');
  const more = items.length > limit ? `<div class="recent-controls"><span class="faint">${showAllRecent ? tr("{0} entries", items.length) : tr("{0} newest of {1}", limit, items.length)}</span><button class="more" type="button" data-recent-all>${showAllRecent ? tr("show only the newest {0}", limit) : tr("show all {0}", items.length)}</button></div>` : '';
  return mod('recent', tr("What happened"), tr("Last 14 days, newest first. On the same day, recently updated Cards come first. Click for details"), `${more}<div class="events">${body}</div>`);
}

function blockSafety() {
  const rows = byLastWork(visible()).filter(p => p.facts && p.facts.is_git).map(p => {
    const s = safety(p);
    const unsaved = [s.unsaved ? `<span>${esc(s.unsaved)}</span>` : p.facts.dirty && p.facts.dirty.files === 0 ? tr("Working tree clean") : tr("Working tree state unknown"), s.ahead == null ? tr("Push state unknown") : s.ahead > 0 ? tr("{0} local commits ahead of upstream{1}", s.ahead, s.aheadSince ? `, ${esc(s.aheadSince)}` : '') : tr("No commits ahead of the local upstream state")].join('<br>');
    const ci = CI[s.ci] || CI.unknown;
    return `<tr><td>${plink(p.id)}</td>
      <td>${s.remote ? `<span class="tag">${esc((p.facts.remotes || []).map(r => r.name).join(', ') || tr("Remote configured"))}</span>` : s.remote === false ? noRemoteTag(p) : `<span class="faint">${tr("unknown")}</span>`}</td>
      <td>${unsaved}</td>
      <td>${s.leftovers ? `<span class="faint">${esc(s.leftovers)}</span>` : ''} ${collisionTag(p)}</td>
      <td>${ci[1] ? `<span class="tag ${ci[1]}">${ci[0]}</span>` : `<span class="faint">${ci[0]}</span>`}${s.ciAge ? `<div class="faint">${esc(s.ciAge)}</div>` : ''}</td></tr>`;
  }).join('');
  if (!rows) return '';
  return mod('safety', tr("Git state"), tr("Locally measured facts. A remote does not prove a backup; upstream state is read without fetching."), `<div class="tablewrap"><table class="safety"><thead><tr><th>${tr("Project")}</th><th>Remote</th><th>${tr("Working tree")}</th><th>${tr("Other lines of work")}</th><th>CI</th></tr></thead><tbody>${rows}</tbody></table></div>`);
}

// what the agents did overnight, with the signs that something went wrong
function blockNight() {
  const rows = byLastWork(visible()).filter(p => p.facts && p.facts.night && (p.facts.night.commits || p.facts.night.uncommitted_touched)).map(p => {
    const n = p.facts.night;
    const warn = [n.reverts ? `<span class="tag warn">${plural(n.reverts, 'Revert', 'Reverts')}</span>` : '', (p.facts.ci || {}).state === 'red' ? `<span class="tag bad">${tr("CI failed")}</span>` : '', collisionTag(p)].join('');
    const what = [n.commits ? tr("{0}, +{1}/−{2} lines", plural(n.commits, 'Commit', 'Commits'), num(n.lines_added), num(n.lines_removed)) : '', n.uncommitted_touched ? tr("{0} edited without a commit", plural(n.uncommitted_touched, tr("file"), tr("files"))) : ''].filter(Boolean).join(' · ');
    return `<div class="row"><div class="t">${plink(p.id)}</div><div class="badges">${warn}</div><div class="d">${what}</div></div>`;
  }).join('');
  return mod('night', tr("Since yesterday evening"), rows ? tr("What happened since 18:00 yesterday, across all branches and worktrees") : null, rows ? `<div class="list">${rows}</div>` : `<p class="faint">${tr("Quiet since 18:00 yesterday.")}</p>`);
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
    const dirs = (sp.dirs || []).filter(d => d.ok !== true || d.bytes >= 1e6).slice(0, 4).map(d => `<button class="linkish mono" type="button" data-external="${esc(p.path + '\\' + d.path.replace(/\//g, '\\'))}" title="${tr("Open in Explorer")}">${esc(d.path)}</button> ${d.ok === true && Number.isFinite(d.bytes) ? bytes(d.bytes) : tr("Size unknown")}`).join(' · ');
    return `<div class="row${resting ? ' resting' : ''}"><div class="t">${plink(p.id)}${resting ? ` <span class="tag good">${tr("resting")}</span>` : ''}</div><div class="mono">${sp.ok === true && Number.isFinite(sp.total_bytes) ? bytes(sp.total_bytes) : tr("Size unknown")}</div><div class="d">${dirs}</div></div>`;
  }).join('');
  return mod('space', tr("Disk space"), complete ? tr("{0} in reproducible build and package folders. Measured once a day", bytes(total)) : tr("Total size unknown: not all folders could be fully measured."), `<div class="list">${rows}</div>`);
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
    const adopt = p && target && p.status !== target ? `<div class="taskrow"><span class="task-actions"><button class="linkish task" type="button" data-verdict-status="${esc(target)}" data-project="${esc(p.id)}" title="${tr("Sets your intent for the Project; Agents follow it")}">${tr("Apply “{0}”", esc(STATUS[target]))}</button></span></div>` : '';
    return `<div class="idea"><div class="kind">${tr("Park or revive?")}</div><div class="h"><div class="t">${esc(name)}</div><span class="tag ${v[1]}">${esc(v[0])}</span></div>
      <div class="why">${esc(r.why)}</div>${myNote(r.id)}<div class="foot"><div class="who">${plink(r.project)}</div>${fbButtons(r.id)}</div>${adopt}</div>`;
  };
  const cards = [
    ...(pf.patterns || []).map(s => ideaCard(s, s.projects, null, tr("Patterns"))),
    ...(pf.synergies || []).map(s => ideaCard(s, s.projects, null, tr("Connection"))),
    ...(pf.park_or_revive || []).map(verdict),
  ].filter(Boolean);
  return cards.length ? mod('across', tr("Across all Projects"), tr("What becomes visible when you look at them together"), `<div class="cards3">${cards.join('')}</div>`) : '';
}


function blockWeek() {
  const r = (S.portfolio || {}).week_review;
  if (!r || r.week !== S.review_week) return '';
  const list = xs => (xs || []).length ? `<ul class="bullets">${xs.map(x => `<li>${esc(x)}</li>`).join('')}</ul>` : `<p class="faint">${tr("nothing")}</p>`;
  return mod('week', tr("Week {0} review", Number(r.week.split('-W')[1])), r.time_note || null, `<div class="weekgrid"><div><h3>${tr("Completed")}</h3>${list(r.done)}</div><div><h3>${tr("Left unfinished")}</h3>${list(r.stalled)}</div></div>`);
}

function missingCardsNote() {
  const missing = visible().filter(p => !p.card);
  if (!missing.length) return '';
  return `<div class="note" style="margin-top:24px">${tr("{0} no assessment yet: {1}. The next Agent Run writes it.", missing.length === 1 ? tr("One Project has") : tr("{0} Projects have", missing.length), missing.map(p => plink(p.id)).join(', '))} <button class="linkish" type="button" data-open-settings>${tr("How do I set up the Run?")}</button></div>`;
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
    if (p.facts?.error === 'path_missing') return [{ type: 'problem', p, score: 75, title: tr("Project folder missing – check the path"), label: tr("Folder missing") }];
    const s = safety(p);
    // a risk that keeps growing for a week is news even when nobody asks about it
    const growing = ['uncommitted', 'unpushed'].map(kind => streak(p, kind)).filter(r => r && r.trend === 'growing' && r.days >= 7 && p.status === 'active')
      .map(r => ({ type: 'problem', p, score: 65, label: tr("Growing"), title: r.kind === 'uncommitted' ? tr("{0} uncommitted files, previously {1}", num(r.now.files), num(r.then.files)) : tr("{0} unpushed commits, previously {1}", num(r.now.commits), num(r.then.commits)), age: streakText({ ...r, trend: null }) }));
    return [...growing, ...(s.ci === 'red' ? [{ type: 'problem', p, score: 70, title: tr("CI reports failed checks"), label: tr("CI failed"), age: s.ciAge }] : [])];
  });
  return [...tasks, ...goals, ...problems, ...questions].sort((a, b) => b.score - a.score);
}
function blockAttention() {
  const items = attentionItems(), questions = items.filter(i => i.type === 'question').length;
  const selected = inspectedTask && (S.tasks || []).find(t => t.id === inspectedTask);
  const inspection = selected ? `<div class="task-inspection"><button type="button" class="linkish" data-task-inspection-close>${tr("Close Task view")}</button>${trackedTask(selected, true)}</div>` : '';
  if (!items.length) return `<section class="morning-attention quiet"><h2>${tr("Nothing is waiting for you right now")}</h2><p class="muted">${tr("No pending decisions, blocked Tasks or results to confirm.")}</p>${inspection}</section>`;
  const row = item => {
    if (item.type === 'task') return `<details class="attention-row" ${foldAttrs(`attention:${item.task.id}`)}><summary><span class="tag ask">${item.task.status === 'blocked' ? tr("Blocked") : tr("Review result")}</span><span class="attention-title">${esc(item.task.title)}</span><span class="faint">#${item.task.id}</span></summary><div class="attention-detail">${trackedTask(item.task)}</div></details>`;
    const label = item.label || (item.type === 'goal' ? (item.p.goal_due < S.today ? tr("Goal overdue") : tr("Goal due today")) : tr("Decision"));
    return `<div class="attention-row"><span class="tag ask">${label}</span><button type="button" class="linkish attention-title" data-attention-project="${esc(item.p.id)}" data-attention-tab="${item.type === 'problem' ? 'history' : 'work'}" data-attention-anchor="${esc(item.q ? `question:${item.q.id}` : item.type === 'problem' ? 'facts' : 'owner')}">${esc(item.title)}</button><span class="attention-project">${esc(item.p.name)}${item.age ? ` · ${esc(item.age)}` : ''}</span></div>`;
  };
  return `<section class="morning-attention"><div class="attention-head"><div><h2>${tr("Needs you")}</h2><p class="muted">${tr("{0} {1} for a decision or review.", items.length, items.length === 1 ? tr("item waiting") : tr("items waiting"))}</p></div>${questions ? `<button class="btn" type="button" data-round-start>${questions === 1 ? tr("Answer decision") : tr("{0} decisions in sequence", questions)}</button>` : ''}</div><div class="attention-list">${(showAllAttention ? items : items.slice(0, 5)).map(row).join('')}</div>${items.length > 5 ? `<button class="more" type="button" data-attention-all>${showAllAttention ? tr("show fewer again") : tr("show all {0}", items.length)}</button>` : ''}${inspection}</section>`;
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
  const greeting = (S.portfolio && S.portfolio.greeting) || tr("Good morning. Watching {0} {1}.", n, n === 1 ? tr("Project") : tr("Projects"));
  return `<div class="handover">
    <div class="greet"><div class="kicker">${tr("Today · {0} {1}", n, n === 1 ? tr("Project") : tr("Projects"))}</div><h2>${esc(greeting)}</h2></div>
    ${blockAttention()}${blockFocus()}${blockChanges()}
    ${missingCardsNote()}
  </div>`;
}


async function saveWarning(project, key, clear = false) {
  const draftKey = `${project}:${key}`, note = clear ? null : (warningDrafts[draftKey] || '').trim();
  if (!clear && !note) { toast(tr("Briefly explain why this is intentional.")); return; }
  try {
    await call('set_warning_ack', { project, key, note });
    warningFor = null; delete warningDrafts[draftKey]; await load(false);
    toast(clear ? tr("This finding is shown as a warning again.") : tr("Saved as an intentional decision. Changed findings will be reported again."));
  } catch (e) { toast(`${tr("Not saved:")} ` + errText(e)); }
}
async function markChangesRead(token) {
  try { await call('mark_changes_read', { token }); await load(false); toast(tr("Reading baseline saved. Newly added changes remain visible.")); }
  catch (e) { toast(`${tr("Reading baseline not saved:")} ` + errText(e)); }
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
