import { tr, locale, localized } from './i18n.js';
import { S, call, errText, render, toast } from './app.js';
import { $, CI, PALETTE, byId, color, esc, fmtDate, foldAttrs, num, openFolds } from './presentation.js';

let statsProject = null, statsFrom = '', statsUntil = '', statsTab = 'numbers';
let statsData = null, statsHistory = null, statsVersions = null, statsDetail = null;
let statsBusy = false, statsRequest = 0, statsDetailRequest = 0;
let statsBasis = {}, statsText = {}, statsBasisMemo = null; // what the shown statistics rest on, per tab

// ---------- statistics and durable content history ----------
const HISTORY_KIND = localized(() => ({ card: tr("Project report"), portfolio: tr("Portfolio report"), project: tr("Project"), idea: tr("Your idea"), proposal: tr("Agent idea"), task: tr("Task\u0004assignment"), feedback: tr("Your reaction"), warning: tr("Intentional decision") }));
const HISTORY_FIELD = localized(() => ({ headline: tr("Current state"), reentry: tr("Next step"), suggestions: tr("Proposals"), waiting_on_you: tr("Questions for you"), open_ends: tr("Open ends"), handbook: tr("Handbook"), phase: 'Phase', text: 'Text', name: 'Name', path: tr("Folder"), status: 'Status', archived: tr("Archived"), status_note: tr("Status note"), goal: tr("Goal"), goal_due: tr("Goal date"), removed_at: tr("Removed on"), value: tr("Rating"), note: tr("Note"), data: tr("Content"), in_report: tr("In current report"), new_apps: tr("New apps"), synergies: tr("Connections"), focus: tr("Focus"), milestones: tr("Milestones"), recent_changes: tr("Recent changes\u0004history") }));
const measured = n => typeof n === 'number' ? num(n) : tr("unknown");
const signed = n => typeof n === 'number' ? `${n > 0 ? '+' : ''}${num(n)}` : '—';
const observationDate = at => at ? new Date(at).toLocaleString(locale()) : tr("Time unknown");
let statsFilterDraft = {}, statsCompareDraft = {};
// Measured facts and saved content each statistics tab rests on. A reload that changed neither keeps the shown data.
function statisticsBasis() {
  if (statsBasisMemo?.state === S && statsBasisMemo.tab === statsTab) return statsBasisMemo.value;
  const projects = S.projects.map(p => statsTab === 'numbers' ? [p.id, p.name, p.status, p.facts_collected_at]
    : [p.id, p.name, p.path, p.status, p.status_note, p.goal, p.goal_due, p.card_updated_at, p.warnings]);
  const tasks = (S.tasks || []).map(t => [t.id, t.status, t.updated_at]);
  const value = JSON.stringify(statsTab === 'numbers' ? [S.today, projects, tasks]
    : [projects, tasks, S.portfolio_updated_at, S.ideas, (S.proposals || []).map(p => [p.id, p.updated_at, p.in_report]), S.feedback]);
  statsBasisMemo = { state: S, tab: statsTab, value };
  return value;
}
const needsStatistics = () => !(statsTab === 'numbers' ? statsData : statsHistory) || statsBasis[statsTab] !== statisticsBasis();
// Reloads keep the shown statistics, drafts and comparison; the next render fetches only what changed underneath.
function invalidateStatistics() { statsBasisMemo = null; }
// A new selection starts over.
function resetStatistics() {
  statsDetailRequest++;
  statsRequest++; statsBusy = false; statsData = null; statsHistory = null; statsVersions = null; statsBasis = {}; statsText = {};
  statsDetail = null; statsFilterDraft = {}; statsCompareDraft = {};
}
async function fetchStatistics() {
  const request = ++statsRequest, tab = statsTab, basis = statisticsBasis(); statsBusy = true;
  let text;
  try {
    if (tab === 'numbers') {
      const result = await call('statistics', { project: statsProject, from: statsFrom || null, until: statsUntil || null });
      if (request !== statsRequest) return;
      text = JSON.stringify(result);
      if (text !== statsText[tab]) statsData = result;
    } else {
      const [history, versions] = await Promise.all([
        call('history', { project: statsProject, entity: null, id: null, before: null }),
        call('document_versions', { project: statsProject }),
      ]);
      if (request !== statsRequest) return;
      text = JSON.stringify([history, versions]);
      if (text !== statsText[tab]) { statsHistory = history; statsVersions = versions; }
    }
  } catch (e) {
    if (request !== statsRequest) return;
    text = `error:${errText(e)}`;
    if (tab === 'numbers') statsData = { error: errText(e) }; else statsHistory = { error: errText(e) };
  }
  if (request !== statsRequest) return;
  const changed = text !== statsText[tab];
  statsText[tab] = text; statsBasis[tab] = basis; statsBusy = false;
  if (S.prefs.style === 'statistics' && (changed || needsStatistics())) render();
}
function metricCurve(samples, value, label, ink) {
  if (!samples.length) return `<p class="faint">${tr("No measurements in this period yet.")}</p>`;
  const values = samples.map(value), numbers = values.filter(n => typeof n === 'number');
  if (!numbers.length) return `<p class="faint">${tr("{0}: no complete measurement yet.", esc(label))}</p>`;
  const times = samples.map(s => Date.parse(s.at)), lo = times.reduce((a, b) => Math.min(a, b), Infinity), span = Math.max(1, times.reduce((a, b) => Math.max(a, b), -Infinity) - lo), max = numbers.reduce((a, b) => Math.max(a, b), 1);
  const points = samples.map((s, i) => typeof values[i] === 'number' ? { s, n: values[i], x: 12 + (times[i] - lo) / span * 476, y: 83 - values[i] / max * 65 } : null);
  const joined = (a, b) => !!a && !!b && a.s.branch === b.s.branch && a.s.path === b.s.path && a.s.code?.method === b.s.code?.method;
  let trace = '', dots = '';
  points.forEach((p, i) => {
    if (!p) return;
    const before = points[i - 1], after = points[i + 1];
    trace += joined(before, p) ? ` L${p.x},${p.y}` : ` M${p.x},${p.y}`;
    // A dot inside an unchanged stretch adds nothing; the line still spans it.
    if (!(joined(before, p) && joined(p, after) && before.n === p.n && after.n === p.n)) dots += `<circle cx="${p.x}" cy="${p.y}" r="3"><title>${esc(observationDate(p.s.at))}: ${num(p.n)} ${esc(label)}</title></circle>`;
  });
  return `<figure class="stat-curve"><figcaption>${esc(label)} <span class="faint">${tr("· maximum {0}", num(numbers.reduce((a, b) => Math.max(a, b), 0)))}</span></figcaption><svg viewBox="0 0 500 110" role="img" aria-label="${tr("{0}: {1} measurement points. Missing measurements break the line.", esc(label), samples.length)}" style="color:${ink}"><path d="M12 85H488" class="stat-axis"/>${trace.includes(' L') ? `<path d="${trace.trim()}" fill="none" stroke="currentColor" stroke-width="2"/>` : ''}<g fill="currentColor">${dots}</g><text x="12" y="105">${esc(fmtDate(samples[0].at))}</text><text x="488" y="105" text-anchor="end">${esc(fmtDate(samples.at(-1).at))}</text></svg></figure>`;
}
// Closed sections hold no rows until they are opened.
const statsFolds = new Map();
function statsFold(id, summary, body) {
  const open = openFolds.has(id);
  if (!open) statsFolds.set(id, body);
  return `<details class="stat-details" ${foldAttrs(id)}><summary>${summary}</summary>${open ? body() : ''}</details>`;
}
document.addEventListener('toggle', e => {
  const body = e.target.open && statsFolds.get(e.target.dataset?.fold);
  if (!body || e.target.childElementCount > 1) return;
  statsFolds.delete(e.target.dataset.fold);
  e.target.insertAdjacentHTML('beforeend', body());
}, true);
function languageBars(languages, total) {
  const rows = (languages || []).filter(l => l.category == null || l.category === 'source').slice().sort((a, b) => b.code - a.code);
  const row = (l, i) => { const share = total > 0 ? 100 * l.code / total : 0;
    return `<div class="language-row"><div><span>${esc(l.language)}</span><b>${num(l.code)} <span class="faint">· ${share > 0 && share < 0.1 ? tr("&lt; 0.1") : share.toLocaleString(locale(), { maximumFractionDigits: 1 })} %</span></b></div><div class="language-track"><span style="width:${share}%;background:${PALETTE[i % PALETTE.length]}"></span></div></div>`; };
  return rows.length ? `<div class="language-bars">${rows.slice(0, 6).map(row).join('')}</div>${rows.length > 6 ? `<details class="stat-details"><summary>${tr("{0} more languages", rows.length - 6)}</summary><div class="language-bars">${rows.slice(6).map((l,i) => row(l,i+6)).join('')}</div></details>` : ''}` : `<p class="faint">${tr("No source code languages recorded yet.")}</p>`;
}
function projectStatistics(p) {
  const c = p.current, code = c?.code, valid = c?.ok === true && code?.ok === true, activity = p.activity || {}, counts = activity.totals;
  const kpi = (label, value) => `<div class="stat-kpi"><span>${label}</span><strong>${value}</strong></div>`;
  const other = (code?.languages || []).filter(l => l.category !== 'source');
  // Only observations whose shown values differ from the one before become rows.
  const changes = [];
  for (const s of p.samples || []) {
    const cells = [esc(s.branch || '—'), s.ok && s.code?.ok ? measured(s.code.source_code) : tr("unknown"), measured(s.dirty?.files), measured(s.ahead), esc(CI[s.ci?.state]?.[0] || tr("unknown"))];
    if (cells.join('\u0000') !== changes.at(-1)?.key) changes.push({ s, cells, key: cells.join('\u0000') });
  }
  const table = (heads, rows) => `<div class="stat-table-wrap"><table class="stat-table"><thead><tr>${heads.map(h => `<th>${h}</th>`).join('')}</tr></thead><tbody>${rows}</tbody></table></div>`;
  return `<section class="mod stat-project"><div class="mod-head"><div><h2>${esc(p.name)}</h2><div class="sub">${c ? tr("Last saved measurement: {0} · {1}", esc(observationDate(c.at)), esc(c.branch || tr("Branch unknown"))) : tr("No measurement up to the selected end date yet")}</div></div><button type="button" class="linkish" data-stats-project-history="${esc(p.id)}">${tr("Change history")}</button></div>
    <div class="stat-kpis">${kpi(tr("Source lines"), valid ? measured(code.source_code) : tr("unknown"))}${kpi(tr("files"), valid ? measured(code.files) : tr("unknown"))}${kpi(tr("Commits in period"), measured(counts?.commits))}${kpi(tr("Active days"), measured(counts?.active_days))}${kpi(tr("Confirmed completions"), measured(p.confirmed_completions))}</div>
    <p class="stat-comparison">${p.comparable ? `<b>${tr("{0} source lines{1}", signed(p.change?.code), typeof p.change?.percent === 'number' ? ` (${signed(Math.round(p.change.percent * 10) / 10)} %)` : '')}</b> ${tr("since {0}", esc(observationDate(p.baseline?.at)))}` : tr("No reliable size comparison yet: two complete measurements with the same branch, folder and counting method are required.")}</p>
    ${!valid ? `<p class="note">${tr("Code measurement is missing or incomplete. No complete measurement is assumed.")}</p>` : ''}
    <div class="stat-columns"><div>${languageBars(valid ? code.languages : [], valid ? code.source_code : 0)}<p class="faint">${tr("Shares by source lines. Documentation and configuration count separately.")}</p></div>${metricCurve(p.samples || [], s => s.ok && s.code?.ok ? s.code.source_code : null, tr("Source lines"), color(p.id))}</div>
    <div class="stat-columns">${metricCurve(p.samples || [], s => s.ok ? s.dirty?.files : null, tr("Uncommitted files"), '#e8870a')}${metricCurve(p.samples || [], s => s.ok ? s.ahead : null, tr("Unpushed commits"), '#5b8def')}</div>
    <div class="stat-kpis">${kpi(tr("Lines added"), measured(counts?.added))}${kpi(tr("Lines removed"), measured(counts?.removed))}${kpi(tr("CI at latest observation"), esc(CI[c?.ci?.state]?.[0] || tr("unknown")))}${kpi(tr("Additional worktrees"), measured(c?.worktrees?.count))}</div>
    <p class="faint">${tr("Activity reconstructed from Git{0}. {1} Commits and code size describe activity and size, not completion.", activity.coverage_from ? tr(": available {0} to {1}", fmtDate(activity.coverage_from), fmtDate(activity.coverage_through)) : tr(": not available yet"), activity.complete ? '' : tr("The selected period is not fully covered."))}</p>
    ${statsFold(`stats:activity:${p.id}`, tr("Daily activity ({0} active days)", (activity.days || []).length), () => table([tr("day"), 'Commits', tr("Added"), tr("Removed")], (activity.days || []).slice().reverse().map(d => `<tr><td>${fmtDate(d.date)}</td><td>${num(d.commits)}</td><td>+${num(d.added)}</td><td>−${num(d.removed)}</td></tr>`).join('')))}
    ${statsFold(`stats:hotspots:${p.id}`, tr("Frequently changed files"), () => `<p class="faint">${tr("Separate analysis period: {0}. Only source files that still exist; documentation and configuration are excluded.", activity.hotspots_from ? tr("{0} to {1}", fmtDate(activity.hotspots_from), fmtDate(activity.hotspots_through)) : tr("unknown"))}</p>${table([tr("file"), tr("Changes"), tr("Added"), tr("Removed")], (activity.hotspots || []).map(f => `<tr><td>${esc(f.path)}</td><td>${num(f.changes)}</td><td>${num(f.added)}</td><td>${num(f.removed)}</td></tr>`).join(''))}`)}
    ${statsFold(`stats:counting:${p.id}`, tr("Counting method and other file statistics"), () => `<p class="faint">${tr("Working tree including local changes; other worktrees are not added to code size. Known dependencies, build output, lockfiles and detected generated files are excluded.")}</p><p class="faint">${tr("Counting method: {0} · {1} excluded · {2} without a supported text language · {3} not fully read.", esc(code?.method || tr("unknown")), measured(code?.excluded_files), measured(code?.unsupported_files), measured(code?.unread_files))}</p>${table([tr("Language / format"), tr("Category"), 'Code', tr("Comments"), tr("Blank lines")], (code?.languages || []).map(l => `<tr><td>${esc(l.language)}</td><td>${({ source: tr("Source code"), documentation: tr("Documentation"), configuration: tr("Configuration") })[l.category] || tr("Other")}</td><td>${num(l.code)}</td><td>${num(l.comments)}</td><td>${num(l.blanks)}</td></tr>`).join(''))}${other.length ? `<p class="faint">${tr("These additional formats do not affect source code language shares.")}</p>` : ''}<h3>${tr("Largest counted files")}</h3>${table([tr("file"), tr("Language / format"), tr("Lines excluding comments")], (code?.largest_files || []).map(f => `<tr><td>${esc(f.path)}</td><td>${esc(f.language)}</td><td>${num(f.code)}</td></tr>`).join(''))}`)}
    ${statsFold(`stats:observations:${p.id}`, tr("Measurements in period ({0} {1})", changes.length, changes.length === 1 ? tr("observation") : tr("observations")), () => `<p class="faint">${tr("A new row appears when a value changes. The first measurement of each day and every change are saved.")}</p>${table([tr("Since"), 'Branch', tr("Source lines"), tr("Uncommitted files"), tr("Unpushed"), 'CI'], changes.slice().reverse().map(({ s, cells }) => `<tr><td>${esc(observationDate(s.at))}</td>${cells.map(c => `<td>${c}</td>`).join('')}</tr>`).join(''))}`)}</section>`;
}
function readableHistory(value) {
  if (value == null) return '—';
  if (typeof value === 'string') { try { return JSON.stringify(JSON.parse(value), null, 2); } catch (_) { return value; } }
  return typeof value === 'object' ? JSON.stringify(value, null, 2) : String(value);
}
function historyDetail() {
  if (!statsDetail) return '';
  if (statsDetail.error) return `<p class="note">${esc(statsDetail.error)}</p>`;
  const changes = statsDetail.event?.changes || statsDetail.changes || [];
  return `<section class="mod stat-diff"><div class="mod-head"><h2>${statsDetail.event ? tr("Saved change") : tr("Version {0} → {1}", statsDetail.from, statsDetail.to)}</h2><button class="linkish" data-stats-close-detail type="button">${tr("Close")}</button></div>${changes.length ? changes.map(c => `<div class="stat-diff-field"><h3>${esc(HISTORY_FIELD[c.field] || c.field)}</h3><div class="stat-columns"><div><b>${tr("Before")}</b><pre>${esc(readableHistory(c.before))}</pre></div><div><b>${tr("After")}</b><pre>${esc(readableHistory(c.after))}</pre></div></div></div>`).join('') : `<p>${tr("No content differences.")}</p>`}</section>`;
}
function renderContentHistory() {
  if (!statsHistory) return `<p class="muted">${tr("Loading saved changes …")}</p>`;
  if (statsHistory.error) return `<p class="note">${esc(statsHistory.error)}</p>`;
  const versions = statsVersions?.versions || [], opts = selected => versions.map(v => `<option value="${v.version}"${v.version === selected ? ' selected' : ''}>${v.version} · ${esc(observationDate(v.created_at))}</option>`).join('');
  return `${versions.length > 1 ? `<section class="mod"><h2>${tr("Compare {0}", statsProject ? tr("Project reports") : tr("Portfolio reports"))}</h2><form id="stats-compare" class="stats-controls"><label>${tr("From version")}<select id="stats-version-from">${opts(statsCompareDraft['stats-version-from'] ?? versions.at(-2).version)}</select></label><label>${tr("To version")}<select id="stats-version-to">${opts(statsCompareDraft['stats-version-to'] ?? versions.at(-1).version)}</select></label><button class="btn" type="submit">${tr("Compare")}</button></form></section>` : ''}${historyDetail()}<section class="mod"><h2>${tr("Saved changes")}</h2><p class="faint">${tr("Newest saves first. Retained historical data is marked; some earlier changes and authors are unknown.")}</p><ol class="stat-journal">${(statsHistory.events || []).map(e => `<li><div><span class="faint">${esc(observationDate(e.at))} · ${esc(HISTORY_KIND[e.entity] || e.entity)}${e.project ? ` · ${esc(byId(e.project)?.name || e.project)}` : ''}${e.origin === 'retained' ? ` ${tr("· retained history")}` : ''}</span><p><b>${esc(e.title || e.entity_id)}</b></p><span class="faint">${({ insert: tr("Created"), update: tr("Changed"), delete: tr("Removed"), saved: tr("Version saved"), baseline: tr("Existing state retained") })[e.operation] || esc(e.operation)} · ${(e.fields || []).map(f => esc(HISTORY_FIELD[f] || f)).join(', ')}</span></div><button type="button" class="linkish" data-history-event="${e.id}">${tr("View change")}</button></li>`).join('') || `<li>${tr("No saved changes yet.")}</li>`}</ol>${statsHistory.next_before ? `<button type="button" class="btn" data-history-more="${statsHistory.next_before}">${tr("Load older changes")}</button>` : ''}</section>`;
}
function renderStatistics() {
  statsFolds.clear();
  if (!statsBusy && needsStatistics()) fetchStatistics();
  const controls = `<form id="stats-filter" class="stats-controls"><label>${tr("Project")}<select id="stats-project"><option value="">${tr("All Projects")}</option>${S.projects.map(p => `<option value="${esc(p.id)}"${p.id === (statsFilterDraft.project ?? statsProject) ? ' selected' : ''}>${esc(p.name)}</option>`).join('')}</select></label>${statsTab === 'numbers' ? `<label>${tr("From")}<input id="stats-from" type="date" value="${esc(statsFilterDraft.from ?? (statsFrom || statsData?.from || ''))}"></label><label>${tr("To")}<input id="stats-until" type="date" value="${esc(statsFilterDraft.until ?? (statsUntil || statsData?.until || S.today))}"></label>` : ''}<button type="submit" class="btn">${tr("Show")}</button></form>`;
  let body;
  if (statsTab === 'history') body = renderContentHistory();
  else if (!statsData) body = `<p class="muted">${tr("Loading measurements …")}</p>`;
  else if (statsData.error) body = `<p class="note">${esc(statsData.error)}</p>`;
  else body = `<section class="mod stat-total"><h2>${statsProject ? tr("Project size") : tr("All Projects at a glance")}</h2><p><strong>${measured(statsData.summary?.source_code)}</strong> ${tr("source lines · {0} of {1} Projects fully measured", num(statsData.summary?.measured_projects), num(statsData.summary?.projects))}</p><p class="faint">${tr("Latest saved observation up to {0}. Different measurement times remain visible for each Project.", esc(fmtDate(statsData.until)))}</p>${languageBars(statsData.summary?.languages, statsData.summary?.source_code)}</section>${(statsData.projects || []).map(projectStatistics).join('')}`;
  return `<div class="statistics-page"><header class="ideas-heading"><h1>${tr("Statistics & history")}</h1><p>${tr("How your Projects evolve – with saved measurements and traceable changes.")}</p></header><nav class="idea-filters" aria-label="${tr("Statistics view")}"><button class="idea-filter" type="button" data-stats-tab="numbers" aria-pressed="${statsTab === 'numbers'}">${tr("Numbers & trends")}</button><button class="idea-filter" type="button" data-stats-tab="history" aria-pressed="${statsTab === 'history'}">${tr("Change history")}</button></nav>${controls}${statsTab === 'numbers' ? `<div class="stat-presets">${[[7, tr("7 days")], [30, tr("30 days")], [90, tr("90 days")], [365, tr("1 year")]].map(([days, title]) => `<button class="linkish" type="button" data-stats-range="${days}">${title}</button>`).join('')}</div>` : ''}${body}</div>`;
}
async function openStatistics(project, tab = 'numbers') {
  statsProject = project; statsTab = tab; resetStatistics();
  S.prefs.style = 'statistics'; render();
  try { await call('set_pref', { key: 'style', value: 'statistics' }); } catch (e) { toast(errText(e)); }
}

function keepStatisticsDraft(e) {
  const key = { 'stats-project': 'project', 'stats-from': 'from', 'stats-until': 'until' }[e.target.id];
  if (key) statsFilterDraft[key] = e.target.value;
  if (e.target.id === 'stats-version-from' || e.target.id === 'stats-version-to') statsCompareDraft[e.target.id] = Number(e.target.value);
}
document.addEventListener('change', keepStatisticsDraft);
document.addEventListener('input', keepStatisticsDraft);

document.addEventListener('submit', e => {
  if (e.target.id === 'stats-filter') {
    e.preventDefault(); statsProject = $('#stats-project').value || null;
    if (statsTab === 'numbers') { statsFrom = $('#stats-from').value; statsUntil = $('#stats-until').value; }
    resetStatistics(); render();
  }
  if (e.target.id === 'stats-compare') {
    e.preventDefault(); const request = statsRequest, detailRequest = ++statsDetailRequest;
    call('compare_documents', { project: statsProject, from: Number($('#stats-version-from').value), to: Number($('#stats-version-to').value) })
      .then(data => { if (request === statsRequest && detailRequest === statsDetailRequest) { statsDetail = data; render(); } })
      .catch(e => { if (request === statsRequest && detailRequest === statsDetailRequest) toast(errText(e)); });
  }
});
document.addEventListener('click', async e => {
  const t = e.target.closest('[data-project-statistics],[data-stats-project-history],[data-stats-tab],[data-history-event],[data-history-more],[data-stats-close-detail],[data-stats-range]');
  if (!t) return;
  if (t.dataset.projectStatistics) return openStatistics(t.dataset.projectStatistics);
  if (t.dataset.statsProjectHistory) return openStatistics(t.dataset.statsProjectHistory, 'history');
  if (t.dataset.statsRange) {
    const end = new Date(`${S.today}T12:00:00Z`), start = new Date(end);
    start.setUTCDate(start.getUTCDate() - Number(t.dataset.statsRange) + 1);
    statsFrom = start.toISOString().slice(0, 10); statsUntil = S.today;
    resetStatistics(); render(); return;
  }
  if (t.dataset.statsTab) { statsTab = t.dataset.statsTab; statsDetail = null; statsDetailRequest++; render(); return; }
  if (t.hasAttribute('data-stats-close-detail')) { statsDetail = null; statsDetailRequest++; render(); return; }
  const request = statsRequest, detailRequest = t.dataset.historyEvent ? ++statsDetailRequest : null;
  t.disabled = true;
  try {
    if (t.dataset.historyEvent) {
      const result = await call('history_event', { id: Number(t.dataset.historyEvent) });
      if (request === statsRequest && detailRequest === statsDetailRequest) statsDetail = result;
    } else if (t.dataset.historyMore) {
      const result = await call('history', { project: statsProject, entity: null, id: null, before: Number(t.dataset.historyMore) });
      if (request === statsRequest) statsHistory = { ...result, events: [...statsHistory.events, ...result.events] };
    }
    if (request === statsRequest && (detailRequest === null || detailRequest === statsDetailRequest)) render();
  } catch (e) { if (request === statsRequest && (detailRequest === null || detailRequest === statsDetailRequest)) toast(errText(e)); } finally { t.disabled = false; }
});

export { invalidateStatistics, renderStatistics };
