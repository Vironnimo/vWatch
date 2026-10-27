import { errorText, tr, locale, setLanguage } from './i18n.js';
import { handleFeedbackClick, handleFeedbackInput, handleFeedbackKey, handleRoundKey, renderRound, round } from './feedback.js';
import { handleTaskClick, handleTaskPreviewClick, renderTaskPreview, taskPreviewKey } from './handoff.js';
import { handleIdeaClick, handleIdeaInput, handleIdeaKey, ideaBucket, ideaCollection, ideaRound, renderIdeaRound, renderIdeas } from './ideas.js';
import { blockAttention, handleAttentionClick, handleOverviewClick, handleWarningInput, markChangesRead, renderHandover, renderRunStatus } from './overview.js';
import { $, collapsedMods, esc, handleFoldToggle, minutesAgo } from './presentation.js';
import { closeProject, handleProjectChange, handleProjectClick, handleProjectInput, handleProjectKey, renderBoard, renderProjectPage } from './projects.js';
import { handleSearchClick, handleSearchKey, renderSearchLanguage } from './search.js';
import { closePanel, handleSettingsChange, handleSettingsClick, handleSettingsInput, handleSettingsKey, panelOpen, renderEmpty, renderFoot, renderPanel } from './settings.js';
import { invalidateStatistics, renderStatistics } from './statistics.js';

let S = null;           // state from the app core

let lastVersion = null; // data_version of the state shown; the watch reloads when it moves on
let loadSeq = 0, shownSeq = 0, loading = 0; // a state never replaces one requested later
let loadRetryAt = 0;
let shown = false;
// Pending choices survive state reads, and settled choices survive reads that
// were already in flight when the save completed.
const prefWrites = new Map(), prefReadGuards = new Map();

function inputFocus() {
  const el = document.activeElement;
  if (!el || !['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName)) return null;
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

// ---------- page ----------
function render() {
  const languageChanged = setLanguage(S.prefs.language);
  if (document.documentElement) document.documentElement.lang = S.prefs.language === 'de' ? 'de' : 'en';
  document.querySelectorAll('[data-i18n]').forEach(el => { el.textContent = tr(el.dataset.i18n); });
  document.querySelectorAll('[data-i18n-title]').forEach(el => { el.title = tr(el.dataset.i18nTitle); });
  document.querySelectorAll('[data-i18n-label]').forEach(el => { el.setAttribute('aria-label', tr(el.dataset.i18nLabel)); });
  const focused = inputFocus();
  const d = new Date(S.today + 'T12:00:00');
  $('#date').textContent = d.toLocaleDateString(locale(), { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
  const collected = S.projects.map(p => p.facts_collected_at).filter(Boolean).sort()[0];
  $('#fresh').textContent = collected ? tr("Git facts measured {0} min ago", minutesAgo(collected)) : '';
  renderRunStatus();
  $('#refresh').hidden = !S.projects.length;
  $('.switch').hidden = false;
  document.querySelectorAll('.switch button').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.style === S.prefs.style)));
  const freshIdeas = ideaCollection().filter(e => ideaBucket(e) === 'new').length, ideasTab = $('.switch [data-style="ideas"]');
  if (ideasTab) ideasTab.innerHTML = tr("Ideas & recommendations{0}", freshIdeas ? ` <span class="nav-count" title="${tr("{0} new proposals awaiting your rating", freshIdeas)}">${freshIdeas}</span>` : '');
  const style = { board: renderBoard, ideas: renderIdeas, statistics: renderStatistics }[S.prefs.style] || renderHandover;
  $('#view').innerHTML = !S.projects.length && (!S.prefs.style || S.prefs.style === 'handover') ? renderEmpty() + blockAttention() : style();
  renderFoot();
  renderProjectPage();
  if (panelOpen) renderPanel(true);
  if (round) renderRound(true);
  if (ideaRound) renderIdeaRound(true);
  renderTaskPreview();
  if (languageChanged) renderSearchLanguage();
  restoreInputFocus(focused);
}

// ---------- calls into the app ----------
const call = (cmd, args) => window.__TAURI__.core.invoke(cmd, args || {});
const errText = e => errorText((e && e.message) || String(e));
let toastTimer = null;
function toast(text, undo) {
  const t = $('#toast');
  t.innerHTML = `<span>${esc(text)}</span>${undo ? `<button type="button" id="undo">${tr("Undo")}</button>` : ''}`;
  t.hidden = false;
  if (undo) $('#undo').onclick = () => { t.hidden = true; undo(); };
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, 5000);
}
// All state-producing requests share the same acceptance order. A refresh
// response carries the snapshot measured on the service side, not its arrival time.
function acceptState(seq, next) {
  if (seq <= shownSeq) return;
  for (const [key, pending] of prefWrites) next.prefs[key] = pending.latest;
  for (const [key, guard] of prefReadGuards) {
    if (prefWrites.has(key)) continue;
    if (seq <= guard.throughSeq) next.prefs[key] = guard.value;
    else prefReadGuards.delete(key);
  }
  shownSeq = seq; S = next; lastVersion = next.data_version ?? null; loadRetryAt = 0;
  if (S.prefs.style === 'handbook') S.prefs.style = 'board';
  invalidateStatistics();
  render();
}
// Explicit Task recovery must propagate a failed read before permitting retry.
async function readState(visit) {
  const seq = ++loadSeq;
  loading++;
  try { acceptState(seq, await call('state', { visit })); }
  finally { loading--; }
}
async function load(visit) {
  const seq = ++loadSeq;
  loading++;
  try {
    const next = await call('state', { visit });
    acceptState(seq, next);
  } catch (e) {
    if (seq > shownSeq) {
      // A failed read is not an acknowledged database version. Retain the
      // last usable page and retry, including when the first read failed.
      loadRetryAt = Date.now() + 10000;
      if (S) toast(`${tr("Refresh failed. The previous state remains visible; vWatch will retry.")} ` + errText(e));
      else $('#view').innerHTML = `<p class="note" style="margin-top:30px">${tr("vWatch could not load its data: {0}. It will retry automatically.", esc(errText(e)))}</p>`;
    }
  } finally { loading--; }
  if (!shown) { shown = true; call('ready').catch(() => {}); }
}
const REFRESH_MS = 20 * 60 * 1000;
let refreshing = false, lastRefresh = 0;
async function refreshFacts() {
  if (refreshing || !S || !S.projects.length) return;
  const seq = ++loadSeq;
  refreshing = true; lastRefresh = Date.now();
  const b = $('#refresh');
  b.disabled = true; b.textContent = tr("Measuring …");
  try {
    const next = await call('refresh');
    acceptState(seq, next);
  } catch (e) { toast(`${tr("Measurement failed:")} ` + errText(e)); }
  b.disabled = false; b.textContent = tr("Measure Git facts");
  refreshing = false;
}
// Facts every watched project measured within the refresh interval, by the window or an agent's run.
const factsFresh = () => S.projects.filter(p => p.status !== 'archived')
  .every(p => p.facts_collected_at && Date.now() - Date.parse(p.facts_collected_at) < REFRESH_MS);
// Measures only while the window is visible, at most once per interval, and not when the facts are fresh.
function refreshWhenDue() {
  if (!S || document.hidden || Date.now() - lastRefresh < REFRESH_MS || factsFresh()) return;
  refreshFacts();
}
// Saves the owner's reaction to an item (a rating, an answer, "done"); `note` undefined keeps the stored one.

async function setPref(key, value) {
  let pending = prefWrites.get(key);
  const previous = pending?.tail;
  if (!pending) {
    pending = { saved: S.prefs[key], latest: value, count: 0, tail: null };
    prefWrites.set(key, pending);
  }
  pending.latest = value;
  pending.count++;
  S.prefs[key] = value;
  render();
  const save = async () => {
    let failed = false;
    try {
      await call('set_pref', { key, value });
      pending.saved = value;
    } catch (e) {
      failed = true;
    } finally {
      if (--pending.count === 0) {
        prefWrites.delete(key);
        prefReadGuards.set(key, { value: pending.saved, throughSeq: loadSeq });
        S.prefs[key] = pending.saved;
        render();
      }
    }
    if (failed) toast(tr("Setting was not saved."));
  };
  pending.tail = previous ? previous.then(save) : save();
  await pending.tail;
}
async function toggleModule(id) {
  const collapsed = collapsedMods();
  if (collapsed.has(id)) collapsed.delete(id); else collapsed.add(id);
  await setPref('hidden_modules', [...collapsed]);
  $(`[data-mod-toggle="${CSS.escape(id)}"]`)?.focus({ preventScroll: true });
}
// the owner takes over an agent's park/archive/revive verdict as their own intent

$('#refresh').addEventListener('click', refreshFacts);

// Reload when the database moved past the state shown: an agent saved a card, or
// a window write was not followed by its own load. Loads and refreshes in flight
// adopt their own version, so the watch waits for them instead of loading twice.
const typing = () => { const a = document.activeElement; return !!a && ((a.tagName === 'INPUT' && a.type !== 'checkbox') || a.tagName === 'TEXTAREA' || a.tagName === 'SELECT'); };
const idle = () => !refreshing && !loading && !typing();
let watching = false;
async function watch() {
  if (watching || !idle()) return;
  watching = true;
  try {
    if (loadRetryAt) { if (Date.now() >= loadRetryAt) await load(false); return; }
    if (!S) return;
    const v = await call('data_version');
    if (lastVersion === null) lastVersion = v;
    else if (v !== lastVersion && idle()) await load(false);
  } catch (e) { /* the next tick asks again */ }
  finally { watching = false; }
}
setInterval(watch, 2500);
setInterval(refreshWhenDue, 60 * 1000);
document.addEventListener('visibilitychange', () => { if (!document.hidden) refreshWhenDue(); });
window.addEventListener('focus', () => { if (S && !typing()) load(true); });


document.addEventListener('click', async e => {
  const t = e.target;
  const nav = t.closest('[data-nav]');
  if (nav) { await setPref('style', nav.dataset.nav); window.scrollTo(0, 0); return; }
  if (handleAttentionClick(e, t) || handleTaskPreviewClick(e, t)) return;
  if ($('#report-status').open && !t.closest('#report-status')) $('#report-status').open = false;
  if (handleIdeaClick(e, t)) return;
  if (handleFeedbackClick(e, t) || handleTaskClick(e, t) || handleOverviewClick(e, t)) return;
  const read = t.closest('[data-changes-read]');
  if (read) { markChangesRead(read.dataset.changesRead); return; }
  if (handleSearchClick(e, t) || handleProjectClick(e, t) || handleSettingsClick(e, t)) return;
  const sw = t.closest('.switch button');
  if (sw) { setPref('style', sw.dataset.style); window.scrollTo(0, 0); return; }
  const toggle = t.closest('[data-mod-toggle]');
  if (toggle) { toggleModule(toggle.dataset.modToggle); return; }
  const copy = t.closest('[data-copy]');
  if (copy) { navigator.clipboard.writeText(copy.dataset.copy).then(() => toast(tr("Copied.")), () => toast(tr("Copy failed."))); return; }
  const ext = t.closest('[data-external]');
  if (ext) call('open_external', { target: ext.dataset.external }).catch(err => toast(errText(err)));
});
document.addEventListener('change', async e => {
  if (!handleSettingsChange(e)) await handleProjectChange(e);
});
document.addEventListener('input', e => {
  handleFeedbackInput(e); handleIdeaInput(e); handleWarningInput(e); handleSettingsInput(e); handleProjectInput(e);
});
document.addEventListener('toggle', handleFoldToggle, true);
document.addEventListener('keydown', e => {
  if (taskPreviewKey(e) || handleProjectKey(e) || handleIdeaKey(e) || handleRoundKey(e)
    || handleSearchKey(e) || handleSettingsKey(e) || handleFeedbackKey(e)) return;
  if (e.key === 'Escape') {
    if ($('#report-status').open) { e.preventDefault(); $('#report-status').open = false; $('#report-status-summary').focus(); }
    else if (panelOpen) closePanel();
    else if (S && S.prefs.style === 'board' && !typing()) closeProject();
  }
});
load(true).then(refreshWhenDue);

export { S, call, errText, inputFocus, load, readState, render, restoreInputFocus, setPref, toast, typing };
