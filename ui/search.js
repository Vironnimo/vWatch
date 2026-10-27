import { tr } from './i18n.js';
import { S, toast, typing } from './app.js';
import { reply } from './feedback.js';
import { ideaCollection, openIdeaResult } from './ideas.js';
import { openTaskResult } from './overview.js';
import { $, byId, dot, esc } from './presentation.js';
import { openProjectTarget, pastReplies } from './projects.js';

let searchOpen = false;
let searchQ = '';
let searchSel = 0;

// ---------- search ----------
const norm = s => String(s).toLowerCase().normalize('NFD').replace(/[\u0300-\u036f]/g, '');
let searchSnapshot = null; // selection always refers to the results the owner can see
const searchIdentity = value => JSON.stringify(value, (_key, item) => item && typeof item === 'object' && !Array.isArray(item)
  ? Object.fromEntries(Object.entries(item).sort(([a], [b]) => a.localeCompare(b))) : item);
function searchEntries() {
  const out = [];
  for (const p of S.projects) {
    const add = (whereKey, text, tab, anchor, source = text) => { if (text) out.push({ id: p.id, name: p.name, whereKey, where: tr(whereKey), text: String(text), tab: tab || 'work', anchor, identity: searchIdentity([p.id, whereKey, source]) }); };
    add('Name', p.name, 'work', 'name');
    add("Folder", p.path, 'handbook', 'path');
    add("Your intent", [p.status_note, p.goal].filter(Boolean).join(' · '), 'work', 'owner');
    for (const [id, f] of pastReplies(p)) add("Earlier answer", [f.item.title || f.title, f.note].filter(Boolean).join(' · '), 'history', `answer:${id}`, id);
    for (const task of (S.tasks || []).filter(t => (t.projects || []).includes(p.id))) {
      add("Task\u0004assignment", [task.title, task.why, task.done_when, task.result, task.result_evidence].filter(Boolean).join(' · '), 'work', `task:${task.id}`, task.id);
    }
    const c = p.card;
    if (!c) continue;
    add("In brief", c.one_liner, 'work', 'purpose'); add('Stack', c.stack, 'handbook', 'stack'); add("Current state", c.headline, 'work', 'headline');
    add("Project voice", c.personality, 'work', 'voice');
    add("Next step", c.reentry && [c.reentry.next_step, c.reentry.why, c.reentry.done_when].filter(Boolean).join(' · '), 'work', 'reentry');
    add("Everyday use", c.usage && [c.usage.summary, c.usage.evidence].filter(Boolean).join(' · '), 'work', 'usage');
    if (c.vision) add("Vision and progress", [c.vision.statement, c.vision.progress_note].filter(Boolean).join(' · '), 'history', 'vision');
    c.features.forEach((x, i) => add("Can do", x, 'handbook', `feature:${i}`, x));
    c.recent_changes.forEach((x, i) => add("Recent changes", x.detail ? `${x.title} – ${x.detail}` : x.title, 'history', `recent:${i}`, x));
    c.milestones.forEach((x, i) => add("Milestone", x.title, 'history', `milestone:${i}`, x));
    c.open_ends.forEach((x, i) => add("Open end", [x.title, x.detail, x.source].filter(Boolean).join(' · '), 'work', `open:${i}`, x));
    c.waiting_on_you.forEach(x => {
      add("Question for you", [x.title, x.source, ...(x.options || []).map(o => o.answer)].filter(Boolean).join(' · '), 'work', `question:${x.id}`, x.id);
      const answered = reply(x.id);
      if (answered && answered.note) add("Your answer", answered.note, 'work', `question:${x.id}`, x.id);
    });
    c.suggestions.forEach(x => add("Proposal", [x.title, x.why, x.evidence, x.done_when].filter(Boolean).join(' · '), 'work', `suggestion:${x.id}`, x.id));
    c.health_notes.forEach((x, i) => add("Health", x, 'history', `health:${i}`));
    (c.decisions || []).forEach((x, i) => add("Decision", x, 'work', `decision:${i}`));
    c.cross_project_hints.forEach((x, i) => add("Connection", x, 'work', `connection:${i}`));
    const h = c.handbook || {};
    (h.run || []).forEach((x, i) => add("Handbook · Start", `${x.what}: ${x.how}`, 'handbook', `hb-run:${i}`, x));
    if (h.installed) add("Handbook · Installed", `${h.installed.version} ${h.installed.open || ''} ${h.installed.path}`, 'handbook', 'hb-installed', h.installed);
    add("Handbook · Structure", h.architecture, 'handbook', 'hb-architecture');
    (h.places || []).forEach((x, i) => add("Handbook · Where things are", `${x.path} – ${x.what}`, 'handbook', `hb-place:${i}`, x));
    (h.read_first || []).forEach((x, i) => add("Handbook · Read first", `${x.path} – ${x.why}`, 'handbook', `hb-read:${i}`, x));
    (h.needs || []).forEach((x, i) => add("Handbook · Needs", x, 'handbook', `hb-need:${i}`));
    (h.terms || []).forEach((x, i) => add("Handbook · Term", `${x.term}: ${x.meaning}`, 'handbook', `hb-term:${i}`, x));
  }
  for (const idea of ideaCollection()) {
    const note = S.feedback[idea.s.id]?.note;
    if (note && ['up', 'later', 'down'].includes(S.feedback[idea.s.id]?.value)) {
      const project = idea.project && byId(idea.project);
      out.push({ ideaId: idea.s.id, identity: searchIdentity(['idea-note', idea.s.id]), nameGenerated: !project, name: project ? project.name : 'Ideas', where: 'Your note', text: `${idea.s.title} · ${note}` });
    }
    if (idea.project && !idea.retained && !idea.own) continue;
    out.push({ ideaId: idea.s.id, identity: searchIdentity(['idea', idea.s.id]), nameGenerated: true, name: idea.own ? 'Your ideas' : 'Ideas', where: idea.own ? 'Own idea' : 'Proposal', text: [idea.s.title, idea.s.why, idea.s.pitch, idea.s.evidence].filter(Boolean).join(' · ') });
  }
  for (const task of (S.tasks || []).filter(t => !(t.projects || []).some(id => byId(id)))) {
    out.push({ taskId: task.id, identity: searchIdentity(['task', task.id]), nameGenerated: true, name: 'Tasks', where: 'Task\u0004assignment', text: [task.title, task.why, task.done_when, task.result, task.result_evidence].filter(Boolean).join(' · ') });
  }
  return out;
}
function searchResults() {
  const words = norm(searchQ).split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  return searchEntries()
    .map(e => ({ e, hay: norm(`${e.name} ${e.text}`) }))
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
  if (!searchSnapshot || searchSnapshot.query !== searchQ) searchSnapshot = { query: searchQ, results: searchResults() };
  const res = searchSnapshot.results;
  searchSel = Math.min(searchSel, Math.max(0, res.length - 1));
  box.innerHTML = !searchQ.trim() ? `<p class="faint sempty">${tr("Searches Projects, handbooks, Tasks, ideas, your answers and rating notes. A result opens its source.")}</p>`
    : !res.length ? `<p class="faint sempty">${tr("Nothing found.")}</p>`
    : res.map((r, i) => `<button class="sres" type="button" data-sres="${i}" aria-selected="${i === searchSel}"><span class="w">${r.id ? dot(r.id) : ''}<b>${esc(r.nameGenerated ? tr(r.name) : r.name)}</b> · ${esc(tr(r.whereKey || r.where))}</span><span class="x">${highlight(r.text)}</span></button>`).join('');
  const sel = box.querySelector('[aria-selected="true"]');
  if (sel) sel.scrollIntoView({ block: 'nearest' });
}
function renderSearchLanguage() {
  if (!searchOpen) return;
  $('#search-root').innerHTML = `<div class="scrim" data-search-close></div><div class="search" role="dialog" aria-modal="true" aria-label="${tr("Search")}">
    <input type="search" id="search-q" placeholder="${tr("Search, e.g. login, SQLite, port")}" value="${esc(searchQ)}" aria-label="${tr("Search term")}" autocomplete="off">
    <div class="sresults"></div><div class="shint faint">${tr("↑ ↓ select · Enter opens source · Esc closes")}</div></div>`;
  renderSearchResults();
}
function openSearch() {
  if (!S) return;
  searchSnapshot = null;
  searchOpen = true;
  renderSearchLanguage();
  const input = $('#search-q');
  input.focus();
  input.select();
}
const closeSearch = () => { searchOpen = false; searchSnapshot = null; $('#search-root').innerHTML = ''; };
function pickResult(btn) {
  if (!btn) return;
  const selected = (searchOpen ? searchSnapshot?.results || [] : searchResults())[Number(btn.dataset.sres)];
  if (!selected) return;
  // Array positions are presentation anchors, not item identities. Reconcile
  // the visible hit with the current document before choosing its new anchor.
  const matches = searchEntries().filter(entry => entry.identity === selected.identity);
  if (matches.length !== 1) { toast(tr("Result is outdated or ambiguous. Open the search again.")); return; }
  const result = matches[0];
  closeSearch();
  if (result.ideaId) { openIdeaResult(result.ideaId); return; }
  if (result.taskId != null) { openTaskResult(result.taskId); return; }
  openProjectTarget(result.id, result.tab, result.anchor);
}


document.addEventListener('input', e => { if (e.target.id === 'search-q') { searchQ = e.target.value; searchSel = 0; renderSearchResults(); } });

function handleSearchClick(e, t) {
  if (t.closest('[data-search]')) { openSearch(); return true; }
  if (t.closest('[data-search-close]')) { closeSearch(); return true; }
  const sr = t.closest('[data-sres]');
  if (sr) { pickResult(sr); return true; }
  return false;
}

function handleSearchKey(e) {
  if (searchOpen) {
    const n = $('#search-root').querySelectorAll('.sres').length;
    if (e.key === 'Escape') { e.preventDefault(); closeSearch(); }
    else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); if (n) { searchSel = (searchSel + (e.key === 'ArrowDown' ? 1 : n - 1)) % n; renderSearchResults(); } }
    else if (e.key === 'Enter') { e.preventDefault(); pickResult($('#search-root .sres[aria-selected="true"]')); }
    return true;
  }
  if ((e.key === 'k' && (e.ctrlKey || e.metaKey)) || (e.key === '/' && !typing())) { e.preventDefault(); openSearch(); return true; }
  return false;
}

export { handleSearchClick, handleSearchKey, renderSearchLanguage };
