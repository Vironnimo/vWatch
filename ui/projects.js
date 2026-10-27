import { tr, locale, localized } from './i18n.js';
import { S, call, errText, load, render, setPref, toast } from './app.js';
import { openQs, question } from './feedback.js';
import { blockTasks, taskBtn } from './handoff.js';
import { PROPOSAL_CATEGORIES, ideaCard, newestIdeas, proposalCategory } from './ideas.js';
import { blockAcross, blockNight, blockProjects, blockRecent, blockSafety, blockSpace, blockWeek, byAttention, collisionText, collisions, noRemoteTag, plural, projectTasks, releaseText, safety, warningControls } from './overview.js';
import { $, CI, KIND, PHASE, STATUS, ago, byId, bytes, color, days, daysSince, dmod, dot, esc, fmtDate, fmtShort, foldAttrs, lastWork, minutesAgo, num, shownRow, spark, visible, weekly } from './presentation.js';
import { renderEmpty } from './settings.js';

const projectUnread = p => p.card_unread === true;
function cardFreshness(p) {
  if (!p.card) return `<span class="report-freshness">${tr("No report yet")}</span>`;
  const pending = p.pending_owner_replies > 0;
  const date = p.card_updated_at ? tr("Report from {0}", fmtDate(p.card_updated_at)) : tr("Report date unknown");
  return `<span class="report-freshness">${date}${pending ? ` ${tr("· Your answer awaits the next report")}` : p.card_outdated ? ` ${tr("· Project changed since then")}` : ''}</span>`;
}
const phasePill = p => p.card ? `<span class="pill ${esc(p.card.phase)}">${esc(PHASE[p.card.phase] || p.card.phase)}</span>` : `<span class="pill unknown">${tr("no Card")}</span>`;
const projectValue = (p, field) => (projectDrafts[p.id] || {})[field] ?? p[field] ?? '';
const clearProjectDraft = (id, field, saved) => {
  const d = projectDrafts[id];
  if (d && d[field] === saved) delete d[field];
};

function goalLine(p) {
  if (!p.goal) return '';
  const left = p.goal_due ? -daysSince(p.goal_due) : null;
  const when = left == null ? '' : left < 0 ? ` · <span class="bad-t">${tr("{0} {1} overdue", -left, left === -1 ? tr("day") : tr("days\u0004duration"))}</span>` : ` ${tr("· by {0} ({1} {2} left)", fmtShort(p.goal_due), left, left === 1 ? tr("day") : tr("days\u0004duration"))}`;
  return `<div class="goal"><span class="faint">${tr("Goal:")}</span> ${esc(p.goal)}${when}</div>`;
}


const projectDrafts = {}; // unsaved status note and goal fields per project
let openId = null;      // project whose page is open under "Projekte"
const projectEditors = new Set(); // projects whose status and goal fields are open
let projectSearch = null; // the exact search destination, retained during data refresh
let since = null;       // { id, data }: what happened since the owner last opened that project

// ---------- Projects: an overview, then one project page with the project list beside it ----------
function renderBoard() {
  if (!visible().length && !(openId && byId(openId))) return renderEmpty() + blockTasks();
  if (openId && byId(openId)) return `<div class="project-layout">${projectList()}<div id="project-page" class="project-page"></div></div>`;
  return `<div class="project-overview">${blockProjects()}${blockTasks()}<details class="reference-fold" ${foldAttrs('history')}><summary>${tr("History and context")} <span class="faint">${tr("Project updates, review and connections")}</span></summary>${blockRecent()}${blockWeek()}${blockAcross()}</details><details class="reference-fold" ${foldAttrs('facts')}><summary>${tr("Measured Git facts")} <span class="faint">${tr("Working trees, recent activity and disk space")}</span></summary>${blockSafety()}${blockNight()}${blockSpace()}</details></div>`;
}
// the same ranking as the overview, so switching projects keeps the most urgent ones on top
function projectList() {
  const list = byAttention(visible());
  if (!list.some(p => p.id === openId) && byId(openId)) list.push(byId(openId));
  const item = p => {
    const tasks = projectTasks(p), waiting = p.card ? openQs(p.card).length : 0;
    const asks = waiting + tasks.filter(t => ['blocked', 'review'].includes(t.status)).length;
    const marks = `${asks ? `<span class="mark ask" title="${tr("{0} {1} for you", asks, asks === 1 ? tr("item waiting") : tr("items waiting"))}">${asks}</span>` : ''}${safety(p).ci === 'red' ? `<span class="mark bad" title="${tr("CI failed")}">CI</span>` : ''}${projectUnread(p) ? `<span class="mark new" title="${tr("unread")}"></span>` : ''}`;
    const state = p.status !== 'active' ? STATUS[p.status] : p.card ? PHASE[p.card.phase] || p.card.phase : tr("no Card");
    return `<button type="button" class="plist-item" data-open="${esc(p.id)}" aria-current="${p.id === openId ? 'page' : 'false'}">${dot(p.id)}<span class="n">${esc(p.name)}</span><span class="marks">${marks}</span><span class="s">${esc(state)} · ${esc(ago(lastWork(p)))}</span></button>`;
  };
  return `<nav class="project-list" aria-label="${tr("Projects")}"><button type="button" class="plist-back" data-project-overview>${tr("← All Projects")}</button>${list.map(item).join('')}</nav>`;
}

// ---------- handbook: how to start a project, how it is built, where things are ----------
const HB_LISTS = ['run', 'places', 'read_first', 'needs', 'terms'];
const hbEmpty = h => !h || (!h.architecture && !h.installed && !HB_LISTS.some(k => (h[k] || []).length));
// one line per command; lines break only between words, so `-Open` or `build-release.ps1` stay whole
const cmdHtml = how => how.split('; ').map(line => line.split(' ').map(t => t.length <= 32 ? `<span class="nb">${esc(t)}</span>` : esc(t)).join(' ')).join('\n');
function hbParts(h, searchable = false) {
  h = h || {};
  const anchor = key => searchable ? ` data-search-anchor="${key}" tabindex="-1"` : '';
  const kv = (items, key, val, keyClass, group) => items && items.length ? `<dl class="kv">${items.map((x, i) => `<dt class="${keyClass}"${anchor(`${group}:${i}`)}>${esc(x[key])}</dt><dd>${esc(x[val])}</dd>`).join('')}</dl>` : '';
  const i = h.installed;
  const inst = i ? `<div class="hb-inst"${anchor('hb-installed')}>${tr("Installed:")} <b>${esc(i.version)}</b> <button class="linkish mono faint" type="button" data-external="${esc(i.path)}" title="${tr("Open in Explorer")}">${esc(i.path)}</button>${i.open ? `<div>${tr("Open: {0}", esc(i.open))}</div>` : ''}</div>` : '';
  // a URL is opened, a command copied; a URL with a <placeholder> can only be copied and completed
  const act = r => /^https?:\/\/[^\s<]+$/.test(r.how)
    ? `<button class="linkish" type="button" data-external="${esc(r.how)}" aria-label="${tr("Open {0}", esc(r.what))}">${tr("open")}</button>`
    : `<button class="linkish" type="button" data-copy="${esc(r.how)}" aria-label="${tr("Copy {0}", esc(r.what))}">${tr("copy")}</button>`;
  return {
    // what it does on the left, the command at full width beside it, one line per command; copying takes it as written
    run: inst + ((h.run || []).length ? `<div class="hb-runs">${h.run.map((r, i) => `<div class="hb-run"${anchor(`hb-run:${i}`)}><div class="w">${esc(r.what)}${r.folder && r.folder !== '.' ? `<span class="faint mono hb-in">${tr("in {0}", esc(r.folder))}</span>` : ''}</div><div class="cmd"><code class="mono">${cmdHtml(r.how)}</code>${act(r)}</div></div>`).join('')}</div>` : ''),
    arch: h.architecture ? `<p class="hb-arch"${anchor('hb-architecture')}>${esc(h.architecture)}</p>` : '',
    places: kv(h.places, 'path', 'what', 'mono', 'hb-place'),
    read: kv(h.read_first, 'path', 'why', 'mono', 'hb-read'),
    needs: (h.needs || []).length ? `<ul class="bullets">${h.needs.map((n, i) => `<li${anchor(`hb-need:${i}`)}>${esc(n)}</li>`).join('')}</ul>` : '',
    terms: kv(h.terms, 'term', 'meaning', 'term', 'hb-term'),
  };
}
const hbSec = (title, body) => body ? `<div class="hb-sec"><h3>${esc(title)}</h3>${body}</div>` : '';
function handbookBody(h, searchable = false) {
  const x = hbParts(h, searchable);
  return hbSec(tr("How to start it"), x.run) + hbSec(tr("Read first"), x.read) + hbSec(tr("How it is built"), x.arch)
    + hbSec(tr("Where things are"), x.places) + hbSec(tr("Needs"), x.needs) + hbSec(tr("Terms"), x.terms);
}

// what happened since the owner last opened this project, when that was a while ago
function sinceHtml(p) {
  if (!since || since.id !== p.id || !since.data) return '';
  const s = since.data;
  const when = s.days < 28 ? tr("{0} days ago", s.days) : tr("{0} weeks ago", Math.round(s.days / 7));
  const counts = [
    s.commits == null ? '' : s.commits ? `${num(s.commits)} ${s.commits === 1 ? 'Commit' : 'Commits'}` : tr("no commits"),
    s.card_updates ? tr("Card updated {0}×", s.card_updates) : tr("Card unchanged"),
  ].filter(Boolean).join(' · ');
  const lines = [];
  if (s.phase) lines.push(`<li>${tr("Phase: {0} → {1}", esc(PHASE[s.phase.from] || s.phase.from), esc(PHASE[s.phase.to] || s.phase.to || '—'))}</li>`);
  s.milestones.forEach(m => lines.push(`<li><b>${tr("Milestone:")}</b> ${esc(m.title)} <span class="faint mono">${fmtShort(m.date)}</span></li>`));
  s.recent_changes.forEach(ch => lines.push(`<li>${esc(ch.title)} <span class="faint mono">${fmtShort(ch.date)}</span></li>`));
  const body = lines.length ? `<ul class="bullets">${lines.join('')}</ul>` : `<div class="muted">${tr("No further events in saved history.")}</div>`;
  return `<details class="since" ${foldAttrs(`since:${p.id}`)}><summary><b>${tr("Since your last visit")}</b><span class="faint"> · ${when} · ${counts}</span></summary>${body}</details>`;
}

// ---------- project page ----------
const PROJECT_GROUPS = localized(() => ([['work', tr("Resume work")], ['handbook', tr("Start & handbook")], ['history', tr("History & facts")]]));
const pastReplies = p => Object.entries(S.feedback || {}).filter(([id, f]) => f.project_id === p.id
  && f.item?.kind === 'question' && ['answered', 'done'].includes(f.value)
  && !(p.card?.waiting_on_you || []).some(w => w.id === id))
  .sort((a, b) => String(b[1].at || '').localeCompare(String(a[1].at || '')));
// the agent dated an open end when it wrote the card; it keeps ageing until the next card
function openEndAge(p, o) {
  const since = p.card_updated_at ? Math.max(0, daysSince(p.card_updated_at)) : 0;
  const age = o.age_days != null ? o.age_days + since : (shownRow(p.item_persistence, r => r.kind === 'open_end' && r.title === o.title) || {}).days;
  return age >= 3 ? tr("open for {0}", days(age)) : '';
}
function projectPageHtml(p) {
  const c = p.card, f = p.facts || {}, s = safety(p);
  const commits = f.commits || {};
  const anchor = name => `data-search-anchor="${esc(name)}" tabindex="-1"`;
  const section = (key, title, body, sub) => body ? `<div ${anchor(key)}>${dmod(title, body, sub)}</div>` : '';
  const editing = projectEditors.has(p.id);
  const statusBar = `<div class="project-intent" ${anchor('owner')}><div class="intent-summary"><span>${tr("Your intent:")} <b>${esc(STATUS[p.status] || p.status)}</b></span><button class="linkish" type="button" data-project-intent-edit aria-expanded="${editing}" aria-controls="project-owner-fields">${editing ? tr("Close editor") : tr("Edit status and goal")}</button></div>${p.status_note ? `<p class="muted">${esc(p.status_note)}</p>` : ''}${goalLine(p)}
    <div id="project-owner-fields"${editing ? '' : ' hidden'}><div class="statusbar"><span class="seg" role="group" aria-label="Status">${Object.entries(STATUS).map(([k, l]) => `<button type="button" data-status="${k}" aria-pressed="${p.status === k}">${l}</button>`).join('')}</span><input id="status-note" type="text" placeholder="${tr("Note to Agents, e.g. why parked")}" value="${esc(projectValue(p, 'status_note'))}" aria-label="${tr("Note to Agents")}"></div><div class="statusbar goalbar"><span class="faint">${tr("Your goal:")}</span><input id="goal-text" type="text" placeholder="${tr("e.g. release v1.0, bug fixes only")}" value="${esc(projectValue(p, 'goal'))}" aria-label="${tr("Your goal")}"><input id="goal-due" type="date" value="${esc(projectValue(p, 'goal_due'))}" aria-label="${tr("By when")}" title="${tr("By when (optional)")}"></div><p class="faint edit-hint">${tr("Changes are saved when you leave the field.")}</p></div></div>`;
  const factsKv = f.is_git ? `<dl class="kv">
    <dt>Commits</dt><dd class="mono">${tr("{0} total · 7 d: {1} · 30 d: {2} · since {3}", num(commits.total), num(commits.last_7d), num(commits.last_30d), commits.first ? fmtDate(commits.first) : '—')}</dd>
    <dt>Branch</dt><dd class="mono">${esc(f.branch || '—')}${f.upstream ? ` → ${esc(f.upstream)}` : ''}${f.ahead ? ` ${tr("· {0} ahead", f.ahead)}` : ''}${f.behind ? ` ${tr("· {0} behind", f.behind)}` : ''}</dd>
    <dt>Remote</dt><dd>${f.has_remote ? esc((f.remotes || []).map(r => `${r.name}: ${r.url}`).join(', ')) : noRemoteTag(p)}</dd>
    <dt>${tr("Local changes")}</dt><dd>${s.unsaved ? esc(s.unsaved) : tr("no uncommitted changes")}</dd>
    <dt>${tr("Left unfinished")}</dt><dd>${esc(s.leftovers || tr("nothing"))}</dd>
    ${releaseText(p) ? `<dt>Release</dt><dd>${esc(releaseText(p))}</dd>` : ''}
    ${collisions(p).length ? `<dt>${tr("Overlaps")}</dt><dd>${collisions(p).map(c => `<div>${esc(collisionText(c))}</div>`).join('')}</dd>` : ''}
    ${f.night && (f.night.commits || f.night.uncommitted_touched) ? `<dt>${tr("Since yesterday evening")}</dt><dd>${[f.night.commits ? `${plural(f.night.commits, 'Commit', 'Commits')}, +${num(f.night.lines_added)}/−${num(f.night.lines_removed)}` : '', f.night.reverts ? plural(f.night.reverts, 'Revert', 'Reverts') : '', f.night.uncommitted_touched ? tr("{0} edited without a commit", plural(f.night.uncommitted_touched, tr("file"), tr("files"))) : ''].filter(Boolean).join(' · ')}</dd>` : ''}
    ${f.space ? `<dt>${tr("Disk space")}</dt><dd>${f.space.ok === true && Number.isFinite(f.space.total_bytes) ? tr("{0} reproducible", bytes(f.space.total_bytes)) : tr("Size unknown – measurement incomplete")}${(f.space.dirs || []).length ? ': ' + f.space.dirs.filter(d => d.ok !== true || d.bytes >= 1e6).slice(0, 4).map(d => `${esc(d.path)} ${d.ok === true && Number.isFinite(d.bytes) ? bytes(d.bytes) : tr("Size unknown")}`).join(' · ') : ''}</dd>` : ''}
    <dt>CI</dt><dd>${esc((CI[s.ci] || CI.unknown)[0])}${f.ci && f.ci.title ? ` · ${esc(f.ci.workflow || '')}: ${esc(f.ci.title)}` : ''}${f.ci && f.ci.url ? ` · <button class="linkish" type="button" data-external="${esc(f.ci.url)}">${tr("Open Run")}</button>` : ''}</dd>
    </dl>` : `<p class="muted">${f.error === 'path_missing' ? tr("The folder no longer exists.") : tr("Not a Git repository, so no Git facts.")}</p>`;
  const r = c && c.reentry || {};
  const u = c && c.usage || {};
  const usage = u.summary ? `<div class="project-usage" ${anchor('usage')}><span class="faint">${tr("Everyday use")}</span><p>${esc(u.summary)}${u.last_used ? ` <span class="muted">${tr("· last used {0}", fmtDate(u.last_used))}</span>` : ''}</p>${u.evidence ? `<div class="muted">${esc(u.evidence)}</div>` : ''}</div>` : '';
  const head = `<header class="project-summary"><h2 class="pname" ${anchor('name')}>${dot(p.id)}${esc(p.name)} ${p.status !== 'active' ? `<span class="pill unknown">${esc(STATUS[p.status])}</span>` : phasePill(p)}${projectUnread(p) ? `<span class="pill new">${tr("unread")}</span>` : ''}</h2>
    ${c ? `<div class="one" ${anchor('purpose')}>${esc(c.one_liner)}</div>` : ''}
    <div class="project-brief">${c ? `<div class="project-current" ${anchor('headline')}><span class="faint">${tr("Current state")}</span><p>${esc(c.headline)}</p>${cardFreshness(p)}</div>` : ''}
    ${r.next_step ? `<div class="project-next" ${anchor('reentry')}><span class="faint">${tr("Next step")}</span><p>${esc(r.next_step)}</p>${r.why ? `<div class="muted">${esc(r.why)}</div>` : ''}${r.done_when ? `<div class="muted">${tr("Done when: {0}", esc(r.done_when))}</div>` : ''}${taskBtn(`reentry:${p.id}`, { projects: [p.id], title: r.next_step, why: r.why, done_when: r.done_when })}</div>` : ''}</div>
    ${usage}
    ${c && c.personality ? `<div class="say" ${anchor('voice')}>„${esc(c.personality)}“</div>` : ''}</header>
    <nav class="project-jump" aria-label="${tr("Sections")}">${PROJECT_GROUPS.map(([key, title]) => `<button type="button" data-jump="${key}">${title}</button>`).join('')}</nav>`;
  let work = statusBar, handbook = '', history = sinceHtml(p);
  if (!c) {
    work += `<p class="note">${tr("No assessment yet. The next Agent Run writes it. Find measured Git facts under “History & facts”.")}</p>`;
    handbook = `<p class="muted">${tr("No handbook yet. The next Agent Run writes it.")}</p>`;
  } else {
    const v = c.vision || {};
    const suggestions = category => newestIdeas(c.suggestions.filter(s => proposalCategory(s) === category).map(s => ({ s, firstSeen: (p.suggestion_first_seen || {})[s.id] }))).map(({ s }) => {
      const content = ideaCard(s, [], p.id, null, { collection: projectSearch?.id === p.id && projectSearch.anchor === `suggestion:${s.id}` });
      return content ? `<div ${anchor(`suggestion:${s.id}`)}>${content}</div>` : '';
    }).join('');
    const hist = (p.history || []).filter((h, i, a) => i === a.length - 1 || h.headline !== a[i + 1].headline);
    const stampOf = at => new Date(at).toLocaleString(locale(), { day: '2-digit', month: '2-digit', year: '2-digit', hour: '2-digit', minute: '2-digit' });
    const histRow = (h, i, a) => `<li><span class="when">${stampOf(h.at)}</span><span class="rail"></span><span class="what">${esc(h.headline)}${a[i + 1] && a[i + 1].phase !== h.phase ? ` <span class="pill ${esc(h.phase)}">${esc(PHASE[h.phase] || h.phase)}</span>` : ''}</span></li>`;
    const chronicle = hist.length > 1 ? `<ol class="timeline chron">${hist.slice(0, 8).map((h, i) => histRow(h, i, hist)).join('')}</ol>${hist.length > 8 ? `<details ${foldAttrs(`chronicle:${p.id}`)}><summary class="more">${tr("+ {0} older", hist.length - 8)}</summary><ol class="timeline chron">${hist.slice(8).map((h, i) => histRow(h, i + 8, hist)).join('')}</ol></details>` : ''}` : '';
    work += section('questions', tr("Waiting for you"), c.waiting_on_you.length ? `<ul class="qs">${c.waiting_on_you.map(w => question(w, true).replace('<li ', `<li ${anchor(`question:${w.id}`)} `)).join('')}</ul>` : '')
      + section('open-ends', tr("Open ends"), c.open_ends.length ? `<div class="list">${c.open_ends.map((o, i) => `<div class="row" ${anchor(`open:${i}`)}><div class="t">${esc(o.title)}${openEndAge(p, o) ? ` <span class="faint q-age">${esc(openEndAge(p, o))}</span>` : ''}</div><span class="tag">${esc(KIND[o.kind] || o.kind)}</span>${o.detail ? `<div class="d">${esc(o.detail)}</div>` : ''}${o.source ? `<div class="d mono">${esc(o.source)}</div>` : ''}<div class="d">${taskBtn(`open:${p.id}:${o.title}`, { projects: [p.id], title: o.title, why: o.detail, evidence: o.source })}</div></div>`).join('')}</div>` : '')
      + ['recommendation', 'idea'].map(category => section(category === 'idea' ? 'suggestions' : 'recommendations', PROPOSAL_CATEGORIES[category], suggestions(category) ? `<div class="cards2">${suggestions(category)}</div>` : '')).join('')
      + section('decisions', tr("Intentional decisions"), (c.decisions || []).length ? `<ul class="bullets">${c.decisions.map((n, i) => `<li ${anchor(`decision:${i}`)}>${esc(n)}</li>`).join('')}</ul>` : '', tr("From the Project's documents"))
      + section('connections', tr("Connections"), c.cross_project_hints.length ? `<ul class="bullets">${c.cross_project_hints.map((n, i) => `<li ${anchor(`connection:${i}`)}>${esc(n)}</li>`).join('')}</ul>` : '');
    handbook = (c.stack ? `<div class="stack" ${anchor('stack')}>${esc(c.stack)}</div>` : '')
      + (hbEmpty(c.handbook) ? `<p class="muted">${tr("No handbook yet. The next Agent Run writes it.")}</p>` : handbookBody(c.handbook, true))
      + (c.features.length ? hbSec(tr("What it can do today"), `<div class="feat">${c.features.map((x, i) => `<span ${anchor(`feature:${i}`)}>${esc(x)}</span>`).join('')}</div>`) : '');
    history += section('recent', tr("Recent changes"), c.recent_changes.length ? `<div class="list">${c.recent_changes.map((ch, i) => ({ ch, i })).sort((a, b) => b.ch.date.localeCompare(a.ch.date)).map(({ ch, i }) => `<div class="row" ${anchor(`recent:${i}`)}><div class="t">${esc(ch.title)}</div><div class="faint mono">${fmtShort(ch.date)}</div>${ch.detail ? `<div class="d">${esc(ch.detail)}</div>` : ''}</div>`).join('')}</div>` : '')
      + section('vision', tr("Vision and progress"), v.statement || v.progress_note || v.progress_pct != null ? `<div>${v.statement ? `<p>${esc(v.statement)}</p>` : ''}${v.progress_pct != null ? `<div class="faint">${tr("Agent assessment: {0} %", Number(v.progress_pct))}</div>` : ''}${v.progress_note ? `<p class="muted">${esc(v.progress_note)}</p>` : ''}</div>` : '')
      + section('milestones', tr("Milestones"), c.milestones.length ? `<ol class="timeline">${c.milestones.map((m, i) => `<li ${anchor(`milestone:${i}`)}><span class="when">${fmtDate(m.date)}</span><span class="rail"></span><span class="what">${esc(m.title)}</span></li>`).join('')}</ol>` : '')
      + section('chronicle', tr("Chronicle"), chronicle, tr("Headline of each Card version, newest first"))
      + section('health', tr("Health"), c.health_notes.length ? `<ul class="bullets">${c.health_notes.map((n, i) => `<li ${anchor(`health:${i}`)}>${esc(n)}</li>`).join('')}</ul>` : '')
      + (c.phase_evidence ? `<p class="muted" ${anchor('phase')}>${esc(c.phase_evidence)}</p>` : '');
  }
  work += `<div ${anchor('tasks')}>${blockTasks(p.id)}</div>`;
  handbook += `<div class="stack" ${anchor('path')}><button class="linkish mono" type="button" data-external="${esc(p.path)}" title="${tr("Open in Explorer")}">${esc(p.path)}</button></div>`;
  const replies = pastReplies(p);
  history += `<button class="btn" type="button" data-project-statistics="${esc(p.id)}">${tr("Open statistics and full history")}</button>`;
  history += section('past-replies', tr("Your earlier answers"), replies.length ? `<ul class="bullets">${replies.map(([id, f]) => `<li ${anchor(`answer:${id}`)}><b>${esc(f.item.title || f.title)}</b><p>${f.value === 'answered' ? tr("Your answer: {0}", esc(f.note)) : tr("Marked as done by you.")}${f.at ? ` <span class="faint">· ${fmtDate(f.at)}</span>` : ''}</p></li>`).join('')}</ul>` : '')
    + warningControls(p) + section('facts', tr("Facts from Git"), factsKv, p.facts_collected_at ? tr("measured {0} min ago", minutesAgo(p.facts_collected_at)) : null)
    + (f.is_git ? `<details class="commit-history" ${foldAttrs(`commits:${p.id}`)}><summary>${tr("View commit activity")}</summary>${spark(weekly(p), color(p.id), 48)}</details>` : '');
  const groups = { work, handbook, history };
  return `<article class="project-detail" aria-label="${esc(p.name)}">${head}${PROJECT_GROUPS.map(([key, title]) => `<section id="project-group-${key}" class="project-group" ${anchor(`group:${key}`)}><h2 class="group-title">${title}</h2>${groups[key]}</section>`).join('')}</article>`;
}

// Selecting a project records a visit; refreshing the page around it does not.
// From the project list, focus stays in the list so ↑/↓ keeps flipping through projects.
function openProject(id, searchTarget = null, fromList = false) {
  if (!byId(id)) return;
  const switching = openId !== id;
  since = null;
  projectSearch = searchTarget;
  openId = id;
  if (S.prefs.style !== 'board') setPref('style', 'board'); else render();
  if (switching && !searchTarget) window.scrollTo(0, 0);
  if (fromList) document.querySelector(`.project-list [data-open="${CSS.escape(id)}"]`)?.focus({ preventScroll: true });
  else if (!searchTarget) $('#project-page [data-search-anchor="name"]')?.focus({ preventScroll: true });
  call('open_project', { id }).then(r => { if (openId === id && r.since) { since = { id, data: r.since }; renderProjectPage(); } }).catch(() => {});
}
function renderProjectPage() {
  const p = openId && byId(openId), root = $('#project-page');
  if (!p || !root || S.prefs.style !== 'board') return;
  if (projectSearch?.digestTarget) Object.assign(projectSearch, digestDestination(p, projectSearch.digestTarget));
  root.innerHTML = projectPageHtml(p);
  if (projectSearch && projectSearch.id === p.id) {
    const anchor = $(`#project-page [data-search-anchor="${CSS.escape(projectSearch.anchor)}"]`) ? projectSearch.anchor : `group:${projectSearch.group || 'work'}`;
    revealProjectTarget(anchor, false);
  }
}
function closeProject() {
  if (!openId) return;
  const id = openId;
  openId = null; projectSearch = null; since = null;
  render();
  window.scrollTo(0, 0);
  $(`[data-open="${CSS.escape(id)}"]`)?.focus({ preventScroll: true });
}
function revealProjectTarget(anchor, focus = true) {
  const target = $(`#project-page [data-search-anchor="${CSS.escape(anchor)}"]`);
  if (!target) return;
  for (let parent = target.parentElement; parent; parent = parent.parentElement) {
    if (parent.tagName === 'DETAILS') parent.open = true;
  }
  for (const detail of target.querySelectorAll('details')) detail.open = true;
  target.classList.add('search-hit');
  if (focus) { target.scrollIntoView({ block: 'center' }); target.focus({ preventScroll: true }); }
}
// `group` names the part of the page that holds the anchor; it is the fallback when the anchor is gone
function openProjectTarget(id, group = 'work', anchor = 'name') {
  if (!byId(id)) return;
  openProject(id, { id, group, anchor });
  if ($(`#project-page [data-search-anchor="${CSS.escape(anchor)}"]`)) revealProjectTarget(anchor);
  else revealProjectTarget(`group:${PROJECT_GROUPS.some(([key]) => key === group) ? group : 'work'}`);
}
// Resolve authored content at click time; list positions can change after a refresh.
function digestDestination(p, target) {
  const c = p.card || {}, normalized = value => String(value ?? '').trim().replace(/\s+/g, ' ').toLowerCase();
  const same = (a, b) => normalized(a) === normalized(b);
  let group = 'work', fallback = 'group:work', anchors = [];
  if (target?.section === 'recent') {
    group = 'history'; fallback = 'recent';
    anchors = (c.recent_changes || []).flatMap((item, i) => ['date', 'title', 'detail'].every(key => same(item[key], target[key])) ? [`recent:${i}`] : []);
  } else if (target?.section === 'open') {
    fallback = 'open-ends';
    anchors = (c.open_ends || []).flatMap((item, i) => same(item.title, target.title) && same(item.source, target.source) ? [`open:${i}`] : []);
  } else if (target?.section === 'decision') {
    fallback = 'decisions';
    anchors = (c.decisions || []).flatMap((item, i) => same(item, target.title) ? [`decision:${i}`] : []);
  } else if (target?.section === 'question') {
    fallback = 'questions';
    anchors = (c.waiting_on_you || []).filter(item => item.id === target.id).map(item => `question:${item.id}`);
    if (!anchors.length && pastReplies(p).some(([id]) => id === target.id)) {
      group = 'history'; fallback = 'past-replies'; anchors = [`answer:${target.id}`];
    }
  }
  return { group, anchor: anchors.length === 1 ? anchors[0] : fallback, exact: anchors.length === 1 };
}
function openDigestTarget(id, target) {
  const p = byId(id);
  if (!p) { toast(tr("This Project is no longer available.")); return; }
  const destination = digestDestination(p, target);
  openProjectTarget(id, destination.group, destination.anchor);
  projectSearch.digestTarget = target;
  if (!destination.exact) toast(tr("The exact entry is no longer available or is ambiguous. Showing its Project section."));
}
function jumpTo(group) {
  const target = $(`#project-group-${group}`);
  if (!target) return;
  target.scrollIntoView({ block: 'start' });
  target.focus({ preventScroll: true });
}
function moveInProjectList(from, step) {
  const items = [...document.querySelectorAll('.project-list .plist-item')];
  const i = items.indexOf(from);
  if (i >= 0 && items.length) openProject(items[(i + step + items.length) % items.length].dataset.open, null, true);
}


async function adoptVerdict(id, status) {
  const p = byId(id);
  if (!p) return;
  try {
    const result = await call('set_status', { id, status, note: p.status_note || '' });
    await load(false);
    const token = typeof result?.undo_token === 'string' ? result.undo_token : null;
    toast(tr("{0}: applied “{1}”.", p.name, STATUS[status]), token ? async () => { try { await call('undo', { token }); } catch (e) { toast(`${tr("Undo failed:")} ` + errText(e)); } await load(false); } : null);
  } catch (e) { toast(`${tr("Status not saved:")} ` + errText(e)); }
}
async function setStatus(status, note) {
  const id = openId;
  try { await call('set_status', { id, status, note }); clearProjectDraft(id, 'status_note', note); await load(false); }
  catch (e) { toast(tr("Status not saved.")); }
}


function handleProjectClick(e, t) {
  const jump = t.closest('[data-jump]');
  if (jump) { jumpTo(jump.dataset.jump); return true; }
  if (t.closest('[data-project-overview]')) { closeProject(); return true; }
  if (t.closest('[data-project-intent-edit]') && openId) {
    if (projectEditors.has(openId)) projectEditors.delete(openId); else projectEditors.add(openId);
    renderProjectPage();
    $('#project-page [data-project-intent-edit]').focus({ preventScroll: true });
    return true;
  }
  const vs = t.closest('[data-verdict-status]');
  if (vs) { adoptVerdict(vs.dataset.project, vs.dataset.verdictStatus); return true; }
  const st = t.closest('.seg [data-status]');
  if (st && openId) { setStatus(st.dataset.status, $('#status-note').value); return true; }
  const op = t.closest('[data-open]');
  if (op) { openProject(op.dataset.open, null, op.classList.contains('plist-item')); }
  return false;
}

async function handleProjectChange(e) {
  if ((e.target.id === 'goal-text' || e.target.id === 'goal-due') && openId) {
    const id = openId, p = byId(id), goal = $('#goal-text').value, due = $('#goal-due').value || null;
    try {
      await call('set_goal', { id, goal, due });
      p.goal = goal.trim(); p.goal_due = p.goal ? due : null;
      clearProjectDraft(id, 'goal', goal); clearProjectDraft(id, 'goal_due', due || '');
      toast(p.goal ? tr("Goal saved. Agents assess the state against it from the next Run.") : tr("Goal removed."));
    } catch (err) { toast(`${tr("Goal not saved:")} ` + errText(err)); }
    return true;
  }
  if (e.target.id === 'status-note' && openId) {
    const id = openId, p = byId(id), note = e.target.value;
    try { await call('set_status', { id, status: p.status, note }); p.status_note = note; clearProjectDraft(id, 'status_note', note); toast(tr("Note saved.")); }
    catch (err) { toast(tr("Note not saved.")); }
  }
  return false;
}

function handleProjectInput(e) {
  const field = { 'status-note': 'status_note', 'goal-text': 'goal', 'goal-due': 'goal_due' }[e.target.id];
  if (field && openId) (projectDrafts[openId] ||= {})[field] = e.target.value;
  return false;
}

function handleProjectKey(e) {
  if (e.target.classList?.contains('plist-item') && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
    e.preventDefault(); moveInProjectList(e.target, e.key === 'ArrowDown' ? 1 : -1); return true;
  }
  return false;
}

export { cardFreshness, closeProject, goalLine, handleProjectChange, handleProjectClick, handleProjectInput, handleProjectKey, openDigestTarget, openProjectTarget, pastReplies, phasePill, projectUnread, renderBoard, renderProjectPage };
