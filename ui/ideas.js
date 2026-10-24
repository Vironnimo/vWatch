import { tr, locale, localized } from './i18n.js';
import { S, call, errText, load, render, setPref, toast } from './app.js';
import { closeRound, drafts, fbButtons, hiddenByRating, myNote, noteFor, noteForm, rated, react, snoozeFor, snoozedUntil } from './feedback.js';
import { TASK_STATUS, taskBtn, trackedTask } from './handoff.js';
import { $, TYPE, byId, dot, esc, fmtDate, foldAttrs, mod, plink, visible } from './presentation.js';

const expanded = { ideas: false };

let ideaDraft = '';
const ownerIdeaDrafts = new Map();
let ideaFilter = 'new';
let ideaCategory = 'idea';
let ideaHighlight = null;
const ideaEditing = new Map(); // retain an open response even if an agent rewrites its source
const restoringIdeaProjects = new Set();

function ideaCard(s, projects, projectId, kind, options = {}) {
  if (noteFor === s.id) return noteForm(s.id, s.title);
  if (!options.collection && ideaClosed(s.id)) return '';
  if (!options.collection && hiddenByRating(s.id)) return '';
  const firstSeen = options.firstSeen || (projectId ? ((byId(projectId) || {}).suggestion_first_seen || {})[s.id] : (S.new_app_first_seen || {})[s.id]);
  const stamp = firstSeen && Number.isFinite(Date.parse(firstSeen)) ? `<time class="faint" datetime="${esc(firstSeen)}" title="${tr("First proposed: {0}", esc(new Date(firstSeen).toLocaleString(locale())))}">${tr("proposed {0}", esc(new Date(firstSeen).toLocaleDateString(locale())))}</time>` : '';
  const task = ideaTask(s.id);
  const assigned = S.idea_states?.[s.id]?.status === 'in_progress';
  const finished = task && S.idea_states?.[s.id]?.status === 'implemented';
  const returned = S.feedback[s.id]?.value === 'later' && snoozedUntil(S.feedback[s.id]) <= S.today;
  const missingProjects = (projects || []).filter(id => !byId(id));
  const action = S.idea_states?.[s.id]?.archived ? '' : finished ? `<span class="task-status" data-status="${esc(task.status)}">${esc(TASK_STATUS[task.status])} · #${task.id}</span>` : !task && missingProjects.length ? ideaProjectRecovery(missingProjects) : taskBtn(s.id || s.title, { projects: task ? task.projects : projectId ? [projectId] : (projects || []), title: s.title, why: s.why || s.pitch, evidence: s.evidence, done_when: s.done_when }, assigned ? task : null);
  const taskDetails = options.collection && task ? `<details class="idea-assignment" ${foldAttrs(`idea-task:${s.id}:${task.id}`)}><summary class="more">${task.status === 'review' ? tr("Review result") : task.status === 'blocked' ? tr("View blocker") : tr("View Task")}</summary>${trackedTask(task, true)}</details>` : '';
  return `<div class="idea${ideaHighlight === s.id ? ' search-hit' : ''}" data-idea-id="${esc(s.id)}" tabindex="-1">${kind ? `<div class="kind">${esc(kind)}${options.retained ? ` ${tr("· retained from an earlier report")}` : ''}</div>` : ''}<div class="h"><div class="t">${s.severity ? `<span class="sev-${esc(s.severity)}">●</span> ` : ''}${esc(s.title)}</div></div>
    ${returned && !task ? `<div class="idea-returned">${tr("Back again · deferred until {0}", fmtDate(snoozedUntil(S.feedback[s.id])))}</div>` : ''}
    <div class="why">${esc(s.why || s.pitch)}</div>
    ${s.because ? `<div class="why"><b>${tr("Because:")}</b> ${esc(s.because)}</div>` : ''}
    ${s.evidence ? `<details ${foldAttrs(`idea-evidence:${s.id}`)}><summary class="more">${tr("Evidence")}</summary><div class="ev">${esc(s.evidence)}</div></details>` : ''}
    ${myNote(s.id)}${ideaStatusActions(s.id)}
    <div class="foot"><div class="who">${(projects || []).map(plink).join('')}${s.type ? `<span class="tag">${esc(TYPE[s.type] || s.type)}</span>` : ''}</div>${fbButtons(s.id)}</div>
    <div class="taskrow">${stamp}${action}</div>${taskDetails}</div>`;
}


const ideaTime = at => Number.isFinite(Date.parse(at)) ? Date.parse(at) : -Infinity;
const newestIdeas = items => [...items].sort((a, b) => ideaTime(b.firstSeen) - ideaTime(a.firstSeen));
const IDEA_FILTERS = localized(() => ({ new: tr("New"), up: tr("Liked"), later: tr("Later"), assigned: tr("In progress"), done: tr("Implemented"), archived: tr("Archived"), down: tr("Rejected"), all: tr("All") }));
const IDEA_KINDS = localized(() => ({ suggestion: tr("From the Project"), new_app: tr("New app"), synergy: tr("Connection"), owner_idea: tr("Your idea") }));
const PROPOSAL_CATEGORIES = localized(() => ({ idea: tr("Ideas"), recommendation: tr("Recommendations") }));
const proposalCategory = (s, kind = 'suggestion') => ['idea', 'recommendation'].includes(s.category) ? s.category
  : ['new_app', 'owner_idea'].includes(kind) || s.type === 'feature' ? 'idea' : 'recommendation';
const categoryEntries = () => ideaCollection().filter(e => proposalCategory(e.s, e.kind) === ideaCategory);
function setIdeaCategory(category) {
  if (!Object.hasOwn(PROPOSAL_CATEGORIES, category)) return;
  ideaCategory = category; ideaHighlight = null; closeIdeaRound(); render();
  $(`[data-idea-category="${category}"]`)?.focus({ preventScroll: true });
}
function ideaProjectRecovery(ids) {
  return `<div class="idea-project-recovery"><p class="faint">${tr("Restore before assigning a new Task:")}</p>${[...new Set(ids)].map(id => `<div><span>${tr("Project no longer watched: {0}", esc(id))}</span> <button class="linkish" type="button" data-idea-restore-project="${esc(id)}"${restoringIdeaProjects.has(id) ? ' disabled' : ''}>${restoringIdeaProjects.has(id) ? tr("Restoring …") : tr("Restore Project")}</button></div>`).join('')}</div>`;
}
async function restoreIdeaProject(id) {
  if (restoringIdeaProjects.has(id)) return;
  restoringIdeaProjects.add(id); render();
  try {
    const result = await call('restore_project', { id });
    await load(false);
    toast(tr("{0} is being watched again.", result.project?.name || id));
  } catch (e) { toast(tr("Project not restored: {0}", errText(e))); }
  finally { restoringIdeaProjects.delete(id); render(); }
}
// The selected Task and guarded idea revision come from one service snapshot.
const ideaTask = id => S.idea_states?.[id]?.task || null;
function ideaBucket(entry) {
  const state = S.idea_states?.[entry.s.id];
  if (state?.archived) return 'archived';
  if (state?.status === 'in_progress') return 'assigned';
  if (state?.status === 'implemented') return 'done';
  return ({ up: 'up', later: 'later', down: 'down' })[rated(entry.s.id)] || 'new';
}
const busyIdeas = new Set();
const ideaClosed = id => S.idea_states?.[id]?.archived || ['in_progress', 'implemented'].includes(S.idea_states?.[id]?.status);
function ideaStatusActions(id) {
  if (!S.idea_states?.[id] && !ideaCollection().some(e => e.s.id === id)) return '';
  const state = S.idea_states?.[id] || {};
  const progress = state.status || 'open';
  const button = (action, label, title) => `<button class="btn" type="button" data-idea-status="${action}" data-idea-target="${esc(id)}" data-idea-revision="${esc(state.revision || '')}" title="${esc(title)}"${busyIdeas.has(id) ? ' disabled' : ''}>${label}</button>`;
  const actions = state.archived ? button('restore', tr("Restore"), tr("Return to the collection; your previous state is preserved"))
    : (progress === 'open' ? button('in_progress', tr("In progress"), tr("Work is already underway, for example after pasting into an Agent")) : '')
      + (progress !== 'implemented' ? button('implemented', tr("Implemented"), tr("You confirm that this idea is implemented")) : '')
      + (progress !== 'open' ? button('open', tr("Open again"), tr("Reset implementation status; the idea keeps its rating")) : '')
      + button('archived', tr("Archive"), tr("Remove from open proposals; rating and active Tasks are preserved"));
  const label = state.archived ? tr("Archived") : progress === 'implemented' ? tr("Implemented") : progress === 'in_progress' ? tr("In progress") : '';
  return `<div class="idea-status-actions" aria-label="${tr("Implementation status")}">${label ? `<span class="tag">${label}</span>` : ''}${actions}</div>`;
}
async function changeIdeaStatus(id, status, revision) {
  if (busyIdeas.has(id)) return;
  busyIdeas.add(id); render();
  try {
    const result = await call('set_idea_status', { id, status, revision });
    await load(false);
    const undo = result.changed && ['archived', 'restore'].includes(status) ? () => changeIdeaStatus(id, status === 'archived' ? 'restore' : 'archived', result.idea_status.revision) : null;
    toast(({ in_progress: tr("Saved under “In progress”."), implemented: tr("Confirmed as implemented."), open: tr("The idea is open again."), archived: tr("Archived. You can restore it under “Archived”."), restore: tr("Restored from the archive.") })[status], undo);
  } catch (e) {
    await load(false);
    toast(e?.code === 'idea_changed' ? tr("The state has changed. Check the updated entry.") : `${tr("State not confirmed:")} ` + errText(e));
  } finally { busyIdeas.delete(id); render(); }
}
function ideaCollection() {
  const entries = new Map(), pf = S.portfolio || {};
  const add = entry => { if (entry.s.id && !entries.has(entry.s.id)) entries.set(entry.s.id, entry); };
  for (const p of visible()) for (const s of (p.card || {}).suggestions || []) add({ s, project: p.id, projects: [p.id], kind: 'suggestion', firstSeen: (p.suggestion_first_seen || {})[s.id] });
  for (const [field, kind, times] of [['new_apps', 'new_app', S.new_app_first_seen], ['synergies', 'synergy', S.synergy_first_seen]]) {
    for (const s of pf[field] || []) add({ s, project: null, projects: s.projects || [], kind, firstSeen: (times || {})[s.id] });
  }
  for (const own of S.ideas || []) {
    const assessment = own.checked === false ? null : (pf.idea_checks || []).find(c => c.idea === own.id);
    add({ s: { id: `idea:${own.id}`, title: own.text }, own, assessment, project: null, projects: assessment?.fits || [], kind: 'owner_idea', firstSeen: own.added_at });
  }
  for (const proposal of S.proposals || []) {
    const project = proposal.scope === '@portfolio' ? null : proposal.scope;
    if (project && !visible().some(p => p.id === project)) continue;
    add({ s: proposal.item, project, projects: project ? [project] : proposal.item.projects || [], kind: proposal.kind, firstSeen: proposal.first_seen, retained: !proposal.in_report });
  }
  for (const [id, f] of Object.entries(S.feedback || {})) {
    const item = f.item;
    if (!item || !IDEA_KINDS[item.kind] || !['up', 'later', 'down'].includes(f.value) || item.kind === 'owner_idea') continue;
    add({ s: { ...item, id, title: item.title || f.title }, project: f.project_id, projects: f.project_id ? [f.project_id] : item.projects || [], kind: item.kind, firstSeen: f.first_seen, retained: true });
  }
  for (const task of S.tasks || []) {
    const source = task.source_snapshot;
    if (!source || !IDEA_KINDS[source.kind] || source.kind === 'owner_idea') continue;
    add({ s: { ...source, id: task.source_id, title: task.title || source.title, why: task.why, evidence: task.evidence, done_when: task.done_when }, project: source.kind === 'suggestion' ? task.projects?.[0] : null, projects: task.projects || [], kind: source.kind, firstSeen: task.source_first_seen, retained: true });
  }
  // An in-progress reply is local UI state, independent of a background rewrite.
  for (const id of [noteFor, snoozeFor].filter(Boolean)) {
    if (entries.has(id)) ideaEditing.set(id, entries.get(id));
    else if (ideaEditing.has(id)) add(ideaEditing.get(id));
  }
  for (const id of ideaEditing.keys()) if (id !== noteFor && id !== snoozeFor) ideaEditing.delete(id);
  return newestIdeas([...entries.values()]);
}
function ideaEntryCard(entry) {
  const options = { collection: true, firstSeen: entry.firstSeen, retained: entry.retained };
  return entry.own ? ownerIdeaCard(entry, options) : ideaCard(entry.s, entry.projects, entry.project, IDEA_KINDS[entry.kind], options);
}
function renderIdeas() {
  const all = ideaCollection(), entries = all.filter(e => proposalCategory(e.s, e.kind) === ideaCategory), matching = entries.filter(e => ideaFilter === 'all' || ideaBucket(e) === ideaFilter);
  const pinned = all.filter(e => [noteFor, snoozeFor].includes(e.s.id) && !matching.includes(e));
  const isIdea = ideaCategory === 'idea', label = PROPOSAL_CATEGORIES[ideaCategory];
  const categories = Object.entries(PROPOSAL_CATEGORIES).map(([key, title]) => {
    const fresh = all.filter(e => proposalCategory(e.s, e.kind) === key && ideaBucket(e) === 'new').length;
    return `<button class="idea-category" type="button" data-idea-category="${key}" aria-pressed="${ideaCategory === key}" aria-controls="idea-collection">${title}${fresh ? ` <span>${tr("{0} new", fresh)}</span>` : ''}</button>`;
  }).join('');
  const freshCount = entries.filter(e => ideaBucket(e) === 'new').length;
  const tabs = Object.entries(IDEA_FILTERS).map(([filter, label]) => {
    const count = filter === 'all' ? entries.length : entries.filter(e => ideaBucket(e) === filter).length;
    return `<button class="idea-filter" type="button" data-idea-filter="${filter}" aria-pressed="${ideaFilter === filter}" aria-controls="idea-collection">${label} <span>${count}</span></button>`;
  }).join('');
  const descriptions = {
    new: tr("Awaiting a decision or back after “Later”. Newest first by first appearance."),
    up: tr("Liked by you. A Task is created when you explicitly assign it."),
    later: tr("Deferred until the chosen day. Then they return under “New”."),
    assigned: tr("Proposals already started or handed to an Agent, including blockers and results for review."),
    done: tr("Confirmed as implemented by you. You can reopen them."),
    archived: tr("Removed from open proposals without changing your rating. You can restore them; active Tasks are preserved."),
    down: tr("Rejected by you. Agents are instructed not to propose these again."),
    all: isIdea ? tr("New features, better workflows, new apps and your own ideas. Newest first.") : tr("Concrete next steps, maintenance and safeguards. Newest first."),
  };
  const empty = { new: tr("No new {0} right now. Find retained proposals in the other filters.", label), up: tr("No proposals liked yet."), later: tr("No deferred proposals."), assigned: tr("No proposals in progress yet."), done: tr("No implemented proposals yet."), archived: tr("No archived proposals."), down: tr("No rejected proposals."), all: tr("No {0} yet.", label) };
  const roundLabel = freshCount === 1 ? tr("Rate new {0}", isIdea ? tr("Idea") : tr("Recommendation")) : tr("Rate {0} new {1} in sequence", freshCount, label);
  return `<div class="ideas-page"><header class="ideas-heading"><h1>${tr("Ideas & recommendations")}</h1><nav class="idea-categories" aria-label="${tr("Proposal category")}">${categories}</nav><p>${isIdea ? tr("What your Projects could do in the future: new features, simpler workflows and new applications.") : tr("What makes sense now: concrete next steps, maintenance and safeguards.")}</p></header>${isIdea ? ideaAddForm() : ''}${freshCount ? `<div class="idea-round-start"><button class="btn primary" type="button" data-iround-start>${roundLabel}</button><span class="faint">${tr("Good, later or reject, one key each. Agents learn from your choices.")}</span></div>` : ''}<nav class="idea-filters" aria-label="${tr("Filter {0}", label)}">${tabs}</nav><p class="idea-filter-description">${descriptions[ideaFilter]}</p>${pinned.length ? `<section class="idea-pending"><h2>${tr("Open input")}</h2><div class="cards2">${pinned.map(ideaEntryCard).join('')}</div></section>` : ''}<div id="idea-collection" class="cards2">${matching.length ? matching.map(ideaEntryCard).join('') : `<p class="faint idea-empty">${empty[ideaFilter]}</p>`}</div></div>`;
}
function setIdeaFilter(filter) {
  if (!Object.hasOwn(IDEA_FILTERS, filter)) return;
  ideaFilter = filter; ideaHighlight = null; render();
  $(`[data-idea-filter="${filter}"]`)?.focus({ preventScroll: true });
}
async function openIdeaResult(id) {
  const entry = ideaCollection().find(e => e.s.id === id);
  if (!entry) { toast(tr("This idea is no longer in the collection.")); return; }
  ideaCategory = proposalCategory(entry.s, entry.kind); ideaFilter = ideaBucket(entry); ideaHighlight = id;
  await setPref('style', 'ideas');
  const card = $(`[data-idea-id="${CSS.escape(id)}"]`);
  card?.scrollIntoView?.({ block: 'center' });
  card?.focus({ preventScroll: true });
}
function blockIdeas() {
  const fromProjects = visible().flatMap(p => ((p.card || {}).suggestions || []).map(s => ({ s, project: p.id, firstSeen: (p.suggestion_first_seen || {})[s.id] })));
  const apps = ((S.portfolio || {}).new_apps || []).map(s => ({ s, project: null, firstSeen: (S.new_app_first_seen || {})[s.id] }));
  const items = newestIdeas([...fromProjects, ...apps].filter(({ s }) => s.id === noteFor || (!hiddenByRating(s.id) && !ideaClosed(s.id))));
  const total = items.length, limit = 6;
  // Keep an open response form in place even when new ideas arrive ahead of it.
  const shown = expanded.ideas ? items : items.filter(({ s }, i) => i < limit || s.id === noteFor || s.id === snoozeFor);
  const cards = shown.map(({ s, project }) => ideaCard(s, project ? [project] : [], project, project ? tr("From the Project") : tr("New app")));
  const count = cards.length < total ? tr("{0} of {1} proposals", cards.length, total) : `${total} ${total === 1 ? tr("Proposal") : tr("Proposals")}`;
  const control = `<div class="recent-controls"><span class="faint">${count}</span>${total > limit ? `<button class="more" type="button" data-expand="ideas" aria-expanded="${expanded.ideas}" aria-controls="ideas-list">${expanded.ideas ? tr("show only the {0} newest", limit) : tr("show all {0}", total)}</button>` : ''}</div>`;
  return cards.length ? mod('ideas', tr("Ideas"), tr("Newest first · proposals from your Projects and new app ideas"), `${control}<div class="cards3" id="ideas-list">${cards.join('')}</div>`) : '';
}

// ideas the owner parks; the next brief says whether and where each one is worth doing
const IDEA_VERDICT = localized(() => ({ build: [tr("standalone app"), 'good'], fold_in: [tr("add to a Project"), 'good'], later: [tr("later"), ''], drop: [tr("rather not"), 'bad'] }));
function ideaAddForm() {
  return `<div class="idea-add"><label for="idea-text">${tr("Save your own idea")}</label><div class="parkform"><input id="idea-text" type="text" value="${esc(ideaDraft)}" maxlength="600" placeholder="${tr("What would you like to build or improve?")}" aria-label="${tr("New idea")}"><button class="btn" type="button" data-idea-add>${tr("Save idea")}</button></div><p class="faint">${tr("The next Agent Run assesses whether and where it is useful.")}</p></div>`;
}
function ownerIdeaCard(entry) {
  const { s, own, assessment: c } = entry, id = s.id, task = ideaTask(id);
  const assigned = S.idea_states?.[id]?.status === 'in_progress';
  const editing = ownerIdeaDrafts.get(own.id);
  if (editing) return `<form class="idea owner-idea-edit" data-owner-idea-form="${own.id}"><label for="owner-idea-text-${own.id}">${tr("Edit your idea")}</label><textarea id="owner-idea-text-${own.id}" data-owner-idea-text="${own.id}" maxlength="600">${esc(editing.text)}</textarea><div class="foot"><button class="btn" type="submit"${editing.busy ? ' disabled' : ''}>${editing.busy ? tr("Saving …") : tr("Save\u0004button")}</button><button class="linkish" type="button" data-owner-idea-cancel="${own.id}">${tr("Cancel")}</button></div></form>`;
  if (noteFor === id) return noteForm(id, s.title);
  const v = c && (IDEA_VERDICT[c.verdict] || [c.verdict, '']);
  const verdict = c ? `<div class="idea-assessment"><span class="faint">${tr("Agent assessment:")}</span> <span class="tag ${v[1]}">${esc(v[0])}</span><div class="why">${esc(c.why)}</div>${(c.fits || []).length ? `<div class="who">${c.fits.map(plink).join('')}</div>` : ''}</div>` : `<div class="faint">${tr("Not assessed yet.")}</div>`;
  const returned = S.feedback[id]?.value === 'later' && snoozedUntil(S.feedback[id]) <= S.today;
  const finished = task && S.idea_states?.[id]?.status === 'implemented';
  const missingProjects = entry.projects.filter(project => !byId(project));
  const action = S.idea_states?.[s.id]?.archived ? '' : finished ? `<span class="task-status" data-status="${esc(task.status)}">${esc(TASK_STATUS[task.status])} · #${task.id}</span>` : !task && missingProjects.length ? ideaProjectRecovery(missingProjects) : taskBtn(id, { projects: task ? task.projects : entry.projects, title: s.title, why: c?.why || '' }, assigned ? task : null);
  const details = task ? `<details class="idea-assignment" ${foldAttrs(`idea-task:${id}:${task.id}`)}><summary class="more">${task.status === 'review' ? tr("Review result") : task.status === 'blocked' ? tr("View blocker") : tr("View Task")}</summary>${trackedTask(task, true)}</details>` : '';
  return `<div class="idea${ideaHighlight === id ? ' search-hit' : ''}" data-idea-id="${esc(id)}" tabindex="-1"><div class="kind">${tr("Your idea")}</div><div class="h"><div class="t">${esc(s.title)}</div><button class="linkish" type="button" data-owner-idea-edit="${own.id}">${tr("edit")}</button><button class="linkish faint" type="button" data-idea-remove="${own.id}" title="${tr("Remove idea")}" aria-label="${tr("Remove idea")}">${tr("remove")}</button></div>${returned && !task ? `<div class="idea-returned">${tr("Back again · deferred until {0}", fmtDate(snoozedUntil(S.feedback[id])))}</div>` : ''}${verdict}${myNote(id)}${ideaStatusActions(id)}<div class="foot"><time class="faint" datetime="${esc(own.added_at)}">${tr("saved {0}", fmtDate(own.added_at))}</time>${fbButtons(id)}</div><div class="taskrow">${action}</div>${details}</div>`;
}
function blockParking() {
  const items = ideaCollection().filter(e => e.own && (e.s.id === noteFor || (!hiddenByRating(e.s.id) && !ideaClosed(e.s.id)))).map(ownerIdeaCard).join('');
  return mod('parking', tr("Your own ideas"), tr("Save what you would like to build or improve"), ideaAddForm() + (items ? `<div class="cards2">${items}</div>` : ''));
}

// the first brief of a week looks back on the week before; shown until the next one

// ---------- idea round: every new idea, one at a time ----------
let ideaRound = null;   // { queue: idea ids, i, stats, busy }
const ideaRatings = () => [['up', tr("Good"), tr("Save for later. A Task is created when you assign it")], ['later', tr("Later"), tr("Returns in {0} days", S.snooze_days || 7)], ['down', tr("Reject"), tr("Agents will not propose this again")]];
const freshIdea = id => { const entry = categoryEntries().find(e => e.s.id === id); return entry && ideaBucket(entry) === 'new' ? entry : null; };
function startIdeaRound() {
  const queue = categoryEntries().filter(e => ideaBucket(e) === 'new').map(e => e.s.id);
  if (!queue.length) return;
  closeRound();
  ideaRound = { queue, i: 0, stats: { up: 0, later: 0, down: 0, skipped: 0 }, busy: false };
  renderIdeaRound();
}
function closeIdeaRound() {
  if (!ideaRound) return;
  ideaRound = null; $('#round-root').innerHTML = '';
}
function renderIdeaRound(keepFocus = false) {
  const root = $('#round-root');
  if (!ideaRound || ideaRound.busy) return;
  const previousInput = $('#idea-round-note');
  const hadFocus = previousInput && document.activeElement === previousInput;
  while (ideaRound.i < ideaRound.queue.length && !freshIdea(ideaRound.queue[ideaRound.i])) ideaRound.i++;
  const n = ideaRound.queue.length, st = ideaRound.stats;
  let body;
  if (ideaRound.i >= n) {
    body = `<h3>${tr("All done.")}</h3><p class="muted">${tr("{0} good, {1} later, {2} rejected, {3} skipped. Agents use this for future proposals.", st.up, st.later, st.down, st.skipped)}</p>
      <div class="actions"><button class="btn primary" type="button" data-iround-close>${tr("Close")}</button></div>`;
  } else {
    const entry = freshIdea(ideaRound.queue[ideaRound.i]), s = entry.s, id = s.id;
    const who = (entry.projects || []).filter(byId).map(pid => `<span class="rproj-name">${dot(pid)}${esc(byId(pid).name)}</span>`).join('');
    const choices = ideaRatings().map(([value, label, effect], i) => `<button type="button" class="ropt" data-iround-rate="${value}"><span class="k mono">${i + 1}</span><span><b>${label}</b><span class="muted">${esc(effect)}</span></span></button>`).join('');
    body = `<div class="rproj"><span class="faint">${esc(IDEA_KINDS[entry.kind] || '')}</span>${who}</div>
      <h3>${esc(s.title)}</h3>${s.why || s.pitch ? `<p>${esc(s.why || s.pitch)}</p>` : ''}${s.because ? `<p class="muted"><b>${tr("Because:")}</b> ${esc(s.because)}</p>` : ''}${entry.assessment ? `<p class="muted"><b>${tr("Agent assessment:")}</b> ${esc(entry.assessment.why)}</p>` : ''}${s.evidence ? `<div class="qsrc mono">${esc(s.evidence)}</div>` : ''}
      ${ideaStatusActions(id)}<div class="ropts">${choices}</div>
      <div class="inrow"><input type="text" id="idea-round-note" data-iround-note="${esc(id)}" value="${esc(drafts[id] || '')}" maxlength="300" placeholder="${tr("Why? Optional; Agents learn from this")}" aria-label="${tr("Why (optional)")}"></div>
      <div class="actions"><button class="btn" type="button" data-iround-skip>${tr("Skip")}</button></div>`;
  }
  root.innerHTML = `<div class="scrim" data-iround-close></div><div class="round" role="dialog" aria-modal="true" aria-label="${tr("Rate {0}", PROPOSAL_CATEGORIES[ideaCategory])}">
    <div class="rhead"><span class="faint">${tr("Rate {0}{1}", PROPOSAL_CATEGORIES[ideaCategory], ideaRound.i < n ? ` ${tr("· {0} of {1}", ideaRound.i + 1, n)}` : '')}</span><button class="linkish" type="button" data-iround-close>${tr("finish")}</button></div>
    <div class="rprog"><span style="width:${(Math.min(ideaRound.i, n) / n * 100).toFixed(1)}%"></span></div>${body}</div>`;
  if (!keepFocus) root.querySelector(ideaRound.i < n ? '[data-iround-rate]' : '[data-iround-close].btn')?.focus();
  else if (hadFocus) $('#idea-round-note')?.focus();
}
async function ideaRoundStep(value) {
  const active = ideaRound;
  if (!active || active.busy) return;
  const id = active.queue[active.i];
  if (!id || !freshIdea(id)) { renderIdeaRound(); return; }
  if (value === 'skip') { active.stats.skipped++; active.i++; renderIdeaRound(); return; }
  const note = (drafts[id] || '').trim();
  active.busy = true;
  document.querySelectorAll('#round-root button:not([data-iround-close]), #idea-round-note').forEach(el => { el.disabled = true; });
  const saved = await react(id, value, note || undefined);
  // A save may finish after the owner closed this round or started another one.
  if (ideaRound !== active) return;
  active.busy = false;
  if (saved) { active.stats[value]++; active.i++; delete drafts[id]; }
  renderIdeaRound();
}


async function saveOwnerIdea(id) {
  const draft = ownerIdeaDrafts.get(id); if (!draft || draft.busy) return;
  draft.busy = true; render();
  try {
    const result = await call('update_idea', { id, text: draft.text, revision: draft.revision });
    ownerIdeaDrafts.delete(id); await load(false);
    const token = typeof result.undo_token === 'string' ? result.undo_token : null;
    toast(tr("Idea saved. The previous version remains in history."), token ? async () => {
      try { await call('undo', { token }); await load(false); } catch (e) { toast(errText(e)); }
    } : undefined);
  } catch (e) { toast(`${tr("Idea not saved:")} ` + errText(e)); }
  finally { draft.busy = false; if (ownerIdeaDrafts.has(id)) render(); }
}

async function addIdea() {
  const el = $('#idea-text'), text = el ? el.value.trim() : '';
  if (!text) { if (el) el.focus(); return; }
  try {
    const result = await call('add_idea', { text });
    if (ideaDraft.trim() === text) ideaDraft = '';
    await load(false);
    if (Number.isInteger(result.id)) await openIdeaResult(`idea:${result.id}`);
    toast(result.already_exists ? tr("This idea was already saved.") : tr("Idea saved. The next Agent Run assesses it."));
  } catch (e) { toast(tr("Not saved: {0}", errText(e))); }
}
async function removeIdea(id) {
  try {
    await call('remove_idea', { id });
    await load(false);
    toast(tr("Idea removed. It remains saved."), async () => {
      try {
        await call('restore_idea', { id });
        await load(false);
        toast(tr("Idea restored."));
      } catch (e) { toast(tr("Not restored: {0}", errText(e))); }
    });
  } catch (e) { toast(tr("Not removed: {0}", errText(e))); }
}

document.addEventListener('submit', e => { if (e.target.dataset?.ownerIdeaForm) { e.preventDefault(); saveOwnerIdea(Number(e.target.dataset.ownerIdeaForm)); } });
function handleIdeaClick(e, t) {
  const ideaTab = t.closest('[data-idea-filter]');
  if (ideaTab) { setIdeaFilter(ideaTab.dataset.ideaFilter); return true; }
  const ideaStatus = t.closest('[data-idea-status]');
  if (ideaStatus) { changeIdeaStatus(ideaStatus.dataset.ideaTarget, ideaStatus.dataset.ideaStatus, ideaStatus.dataset.ideaRevision); return true; }
  const categoryTab = t.closest('[data-idea-category]');
  if (categoryTab) { setIdeaCategory(categoryTab.dataset.ideaCategory); return true; }
  const ideaProject = t.closest('[data-idea-restore-project]');
  if (ideaProject) { restoreIdeaProject(ideaProject.dataset.ideaRestoreProject); return true; }
  const expand = t.closest('[data-expand]');
  if (expand && expand.dataset.expand === 'ideas') { expanded.ideas = !expanded.ideas; render(); $('[data-expand="ideas"]').focus({ preventScroll: true }); return true; }
  if (t.closest('[data-iround-start]')) { startIdeaRound(); return true; }
  if (t.closest('[data-iround-close]')) { closeIdeaRound(); return true; }
  if (ideaRound) {
    const rate = t.closest('[data-iround-rate]');
    if (rate) { ideaRoundStep(rate.dataset.iroundRate); return true; }
    if (t.closest('[data-iround-skip]')) { ideaRoundStep('skip'); return true; }
  }
  if (t.closest('[data-idea-add]')) { addIdea(); return true; }
  const ir = t.closest('[data-idea-remove]');
  if (ir) { removeIdea(Number(ir.dataset.ideaRemove)); return true; }
  const ie = t.closest('[data-owner-idea-edit]');
  if (ie) { const own = S.ideas.find(i => i.id === Number(ie.dataset.ownerIdeaEdit)); if (own) { ownerIdeaDrafts.set(own.id, { text: own.text, revision: own.revision }); render(); } return true; }
  const ic = t.closest('[data-owner-idea-cancel]');
  if (ic) { ownerIdeaDrafts.delete(Number(ic.dataset.ownerIdeaCancel)); render(); return true; }
  return false;
}

function handleIdeaInput(e) {
  if (e.target.id === 'idea-text') ideaDraft = e.target.value;
  if (e.target.dataset?.ownerIdeaText) { const draft = ownerIdeaDrafts.get(Number(e.target.dataset.ownerIdeaText)); if (draft) draft.text = e.target.value; }
  return false;
}

function handleIdeaKey(e) {
  if (e.target.id === 'idea-text' && e.key === 'Enter') { e.preventDefault(); addIdea(); return true; }
  if (ideaRound) {
    if (e.key === 'Escape') { e.preventDefault(); closeIdeaRound(); return true; }
    const value = e.target.id === 'idea-round-note' ? null : ({ 1: 'up', 2: 'later', 3: 'down' })[e.key];
    if (value) { e.preventDefault(); ideaRoundStep(value); }
    return true;
  }
  return false;
}

export { PROPOSAL_CATEGORIES, closeIdeaRound, handleIdeaClick, handleIdeaInput, handleIdeaKey, ideaBucket, ideaCard, ideaCollection, ideaHighlight, ideaProjectRecovery, ideaRound, newestIdeas, openIdeaResult, proposalCategory, renderIdeaRound, renderIdeas, restoreIdeaProject };
