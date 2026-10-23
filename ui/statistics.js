import { S, call, errText, render, toast } from './app.js';
import { $, CI, PALETTE, byId, color, esc, fmtDate, foldAttrs, num, openFolds } from './presentation.js';

let statsProject = null, statsFrom = '', statsUntil = '', statsTab = 'numbers';
let statsData = null, statsHistory = null, statsVersions = null, statsDetail = null;
let statsBusy = false, statsRequest = 0, statsDetailRequest = 0;
let statsBasis = {}, statsText = {}, statsBasisMemo = null; // what the shown statistics rest on, per tab

// ---------- statistics and durable content history ----------
const HISTORY_KIND = { card: 'Projektbericht', portfolio: 'Gesamtbericht', project: 'Projekt', idea: 'Deine Idee', proposal: 'Agentenidee', task: 'Auftrag', feedback: 'Deine Reaktion', warning: 'Bewusste Entscheidung' };
const HISTORY_FIELD = { headline: 'Aktueller Stand', reentry: 'Nächster Schritt', suggestions: 'Vorschläge', waiting_on_you: 'Fragen an dich', open_ends: 'Offene Enden', handbook: 'Handbuch', phase: 'Phase', text: 'Text', name: 'Name', path: 'Ordner', status: 'Status', archived: 'Archiviert', status_note: 'Statusnotiz', goal: 'Ziel', goal_due: 'Zieldatum', removed_at: 'Entfernt am', value: 'Bewertung', note: 'Notiz', data: 'Inhalt', in_report: 'Im aktuellen Bericht', new_apps: 'Neue Apps', synergies: 'Querverbindungen', focus: 'Fokus', milestones: 'Meilensteine', recent_changes: 'Letzte Änderungen' };
const measured = n => typeof n === 'number' ? num(n) : 'unbekannt';
const signed = n => typeof n === 'number' ? `${n > 0 ? '+' : ''}${num(n)}` : '—';
const observationDate = at => at ? new Date(at).toLocaleString('de-DE') : 'Zeitpunkt unbekannt';
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
  if (!samples.length) return `<p class="faint">Noch keine Messungen im Zeitraum.</p>`;
  const values = samples.map(value), numbers = values.filter(n => typeof n === 'number');
  if (!numbers.length) return `<p class="faint">${esc(label)}: noch keine vollständige Messung.</p>`;
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
  return `<figure class="stat-curve"><figcaption>${esc(label)} <span class="faint">· maximal ${num(numbers.reduce((a, b) => Math.max(a, b), 0))}</span></figcaption><svg viewBox="0 0 500 110" role="img" aria-label="${esc(label)}: ${samples.length} Messpunkte. Fehlende Messwerte unterbrechen die Linie." style="color:${ink}"><path d="M12 85H488" class="stat-axis"/>${trace.includes(' L') ? `<path d="${trace.trim()}" fill="none" stroke="currentColor" stroke-width="2"/>` : ''}<g fill="currentColor">${dots}</g><text x="12" y="105">${esc(fmtDate(samples[0].at))}</text><text x="488" y="105" text-anchor="end">${esc(fmtDate(samples.at(-1).at))}</text></svg></figure>`;
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
    return `<div class="language-row"><div><span>${esc(l.language)}</span><b>${num(l.code)} <span class="faint">· ${share > 0 && share < 0.1 ? '&lt; 0,1' : share.toLocaleString('de-DE', { maximumFractionDigits: 1 })} %</span></b></div><div class="language-track"><span style="width:${share}%;background:${PALETTE[i % PALETTE.length]}"></span></div></div>`; };
  return rows.length ? `<div class="language-bars">${rows.slice(0, 6).map(row).join('')}</div>${rows.length > 6 ? `<details class="stat-details"><summary>${rows.length - 6} weitere Sprachen</summary><div class="language-bars">${rows.slice(6).map((l,i) => row(l,i+6)).join('')}</div></details>` : ''}` : '<p class="faint">Noch keine Quellcode-Sprachen erfasst.</p>';
}
function projectStatistics(p) {
  const c = p.current, code = c?.code, valid = c?.ok === true && code?.ok === true, activity = p.activity || {}, counts = activity.totals;
  const kpi = (label, value) => `<div class="stat-kpi"><span>${label}</span><strong>${value}</strong></div>`;
  const other = (code?.languages || []).filter(l => l.category !== 'source');
  // Only observations whose shown values differ from the one before become rows.
  const changes = [];
  for (const s of p.samples || []) {
    const cells = [esc(s.branch || '—'), s.ok && s.code?.ok ? measured(s.code.source_code) : 'unbekannt', measured(s.dirty?.files), measured(s.ahead), esc(CI[s.ci?.state]?.[0] || 'unbekannt')];
    if (cells.join('\u0000') !== changes.at(-1)?.key) changes.push({ s, cells, key: cells.join('\u0000') });
  }
  const table = (heads, rows) => `<div class="stat-table-wrap"><table class="stat-table"><thead><tr>${heads.map(h => `<th>${h}</th>`).join('')}</tr></thead><tbody>${rows}</tbody></table></div>`;
  return `<section class="mod stat-project"><div class="mod-head"><div><h2>${esc(p.name)}</h2><div class="sub">${c ? `Letzter gespeicherter Messstand: ${esc(observationDate(c.at))} · ${esc(c.branch || 'Branch unbekannt')}` : 'Noch kein Messstand bis zum gewählten Enddatum'}</div></div><button type="button" class="linkish" data-stats-project-history="${esc(p.id)}">Änderungsverlauf</button></div>
    <div class="stat-kpis">${kpi('Codezeilen', valid ? measured(code.source_code) : 'unbekannt')}${kpi('Dateien', valid ? measured(code.files) : 'unbekannt')}${kpi('Commits im Zeitraum', measured(counts?.commits))}${kpi('Aktive Tage', measured(counts?.active_days))}${kpi('Bestätigte Abschlüsse', measured(p.confirmed_completions))}</div>
    <p class="stat-comparison">${p.comparable ? `<b>${signed(p.change?.code)} Codezeilen${typeof p.change?.percent === 'number' ? ` (${signed(Math.round(p.change.percent * 10) / 10)} %)` : ''}</b> seit ${esc(observationDate(p.baseline?.at))}` : 'Noch kein belastbarer Größenvergleich: Es braucht zwei vollständige Messungen mit demselben Branch, Ordner und Zählverfahren.'}</p>
    ${!valid ? '<p class="note">Die Codezählung fehlt oder ist unvollständig. Es wird kein vollständiger Messwert angenommen.</p>' : ''}
    <div class="stat-columns"><div>${languageBars(valid ? code.languages : [], valid ? code.source_code : 0)}<p class="faint">Anteile nach Quellcodezeilen. Dokumentation und Konfiguration zählen separat.</p></div>${metricCurve(p.samples || [], s => s.ok && s.code?.ok ? s.code.source_code : null, 'Codezeilen', color(p.id))}</div>
    <div class="stat-columns">${metricCurve(p.samples || [], s => s.ok ? s.dirty?.files : null, 'Dateien ohne Commit', '#e8870a')}${metricCurve(p.samples || [], s => s.ok ? s.ahead : null, 'Commits ohne Push', '#5b8def')}</div>
    <div class="stat-kpis">${kpi('Zeilen hinzugefügt', measured(counts?.added))}${kpi('Zeilen entfernt', measured(counts?.removed))}${kpi('CI beim letzten Stand', esc(CI[c?.ci?.state]?.[0] || 'unbekannt'))}${kpi('Zusätzliche Worktrees', measured(c?.worktrees?.count))}</div>
    <p class="faint">Aktivität aus Git rekonstruiert${activity.coverage_from ? `: verfügbar ${fmtDate(activity.coverage_from)} bis ${fmtDate(activity.coverage_through)}` : ': noch nicht verfügbar'}. ${activity.complete ? '' : 'Der gewählte Zeitraum ist nicht vollständig abgedeckt.'} Commits und Codeumfang beschreiben Aktivität und Größe, keinen Fertigstellungsgrad.</p>
    ${statsFold(`stats:activity:${p.id}`, `Tägliche Aktivität (${(activity.days || []).length} aktive Tage)`, () => table(['Tag', 'Commits', 'Hinzugefügt', 'Entfernt'], (activity.days || []).slice().reverse().map(d => `<tr><td>${fmtDate(d.date)}</td><td>${num(d.commits)}</td><td>+${num(d.added)}</td><td>−${num(d.removed)}</td></tr>`).join('')))}
    ${statsFold(`stats:hotspots:${p.id}`, 'Häufig bearbeitete Dateien', () => `<p class="faint">Eigener Auswertungszeitraum: ${activity.hotspots_from ? `${fmtDate(activity.hotspots_from)} bis ${fmtDate(activity.hotspots_through)}` : 'unbekannt'}. Nur Quelldateien, die es noch gibt; Dokumentation und Konfiguration zählen nicht.</p>${table(['Datei', 'Änderungen', 'Hinzugefügt', 'Entfernt'], (activity.hotspots || []).map(f => `<tr><td>${esc(f.path)}</td><td>${num(f.changes)}</td><td>${num(f.added)}</td><td>${num(f.removed)}</td></tr>`).join(''))}`)}
    ${statsFold(`stats:counting:${p.id}`, 'Zählweise und weitere Dateistatistiken', () => `<p class="faint">Arbeitsverzeichnis einschließlich lokaler Änderungen; weitere Worktrees werden nicht zum Codeumfang addiert. Bekannte Abhängigkeiten, Build-Ausgaben, Lockdateien und erkannte generierte Dateien werden ausgeschlossen.</p><p class="faint">Zählverfahren: ${esc(code?.method || 'unbekannt')} · ${measured(code?.excluded_files)} ausgeschlossen · ${measured(code?.unsupported_files)} ohne unterstützte Textsprache · ${measured(code?.unread_files)} nicht vollständig gelesen.</p>${table(['Sprache / Format', 'Bereich', 'Code', 'Kommentare', 'Leerzeilen'], (code?.languages || []).map(l => `<tr><td>${esc(l.language)}</td><td>${({ source: 'Quellcode', documentation: 'Dokumentation', configuration: 'Konfiguration' })[l.category] || 'Sonstiges'}</td><td>${num(l.code)}</td><td>${num(l.comments)}</td><td>${num(l.blanks)}</td></tr>`).join(''))}${other.length ? '<p class="faint">Diese zusätzlichen Formate verändern die Sprachenanteile des Quellcodes nicht.</p>' : ''}<h3>Größte gezählte Dateien</h3>${table(['Datei', 'Sprache / Format', 'Zeilen ohne Kommentare'], (code?.largest_files || []).map(f => `<tr><td>${esc(f.path)}</td><td>${esc(f.language)}</td><td>${num(f.code)}</td></tr>`).join(''))}`)}
    ${statsFold(`stats:observations:${p.id}`, `Messwerte im Zeitraum (${changes.length} ${changes.length === 1 ? 'Stand' : 'Stände'})`, () => `<p class="faint">Neue Zeile nur, wenn sich einer der Werte ändert. Gespeichert wird je Tag die erste Messung und jede Änderung.</p>${table(['Seit', 'Branch', 'Codezeilen', 'Dateien ohne Commit', 'Ohne Push', 'CI'], changes.slice().reverse().map(({ s, cells }) => `<tr><td>${esc(observationDate(s.at))}</td>${cells.map(c => `<td>${c}</td>`).join('')}</tr>`).join(''))}`)}</section>`;
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
  return `<section class="mod stat-diff"><div class="mod-head"><h2>${statsDetail.event ? 'Gespeicherte Änderung' : `Version ${statsDetail.from} → ${statsDetail.to}`}</h2><button class="linkish" data-stats-close-detail type="button">Schließen</button></div>${changes.length ? changes.map(c => `<div class="stat-diff-field"><h3>${esc(HISTORY_FIELD[c.field] || c.field)}</h3><div class="stat-columns"><div><b>Vorher</b><pre>${esc(readableHistory(c.before))}</pre></div><div><b>Nachher</b><pre>${esc(readableHistory(c.after))}</pre></div></div></div>`).join('') : '<p>Keine inhaltlichen Unterschiede.</p>'}</section>`;
}
function renderContentHistory() {
  if (!statsHistory) return '<p class="muted">Lade gespeicherte Änderungen …</p>';
  if (statsHistory.error) return `<p class="note">${esc(statsHistory.error)}</p>`;
  const versions = statsVersions?.versions || [], opts = selected => versions.map(v => `<option value="${v.version}"${v.version === selected ? ' selected' : ''}>${v.version} · ${esc(observationDate(v.created_at))}</option>`).join('');
  return `${versions.length > 1 ? `<section class="mod"><h2>${statsProject ? 'Projektberichte' : 'Gesamtberichte'} vergleichen</h2><form id="stats-compare" class="stats-controls"><label>Von Version<select id="stats-version-from">${opts(statsCompareDraft['stats-version-from'] ?? versions.at(-2).version)}</select></label><label>Zu Version<select id="stats-version-to">${opts(statsCompareDraft['stats-version-to'] ?? versions.at(-1).version)}</select></label><button class="btn" type="submit">Vergleichen</button></form></section>` : ''}${historyDetail()}<section class="mod"><h2>Gespeicherte Änderungen</h2><p class="faint">Neueste Speichervorgänge zuerst. Übernommene Altbestände sind gekennzeichnet; ihre früheren Änderungen und Verfasser sind teilweise unbekannt.</p><ol class="stat-journal">${(statsHistory.events || []).map(e => `<li><div><span class="faint">${esc(observationDate(e.at))} · ${esc(HISTORY_KIND[e.entity] || e.entity)}${e.project ? ` · ${esc(byId(e.project)?.name || e.project)}` : ''}${e.origin === 'retained' ? ' · Altbestand' : ''}</span><p><b>${esc(e.title || e.entity_id)}</b></p><span class="faint">${({ insert: 'Angelegt', update: 'Geändert', delete: 'Entfernt', saved: 'Version gespeichert', baseline: 'Vorhandener Stand übernommen' })[e.operation] || esc(e.operation)} · ${(e.fields || []).map(f => esc(HISTORY_FIELD[f] || f)).join(', ')}</span></div><button type="button" class="linkish" data-history-event="${e.id}">Änderung ansehen</button></li>`).join('') || '<li>Noch keine gespeicherten Änderungen.</li>'}</ol>${statsHistory.next_before ? `<button type="button" class="btn" data-history-more="${statsHistory.next_before}">Ältere Änderungen laden</button>` : ''}</section>`;
}
function renderStatistics() {
  statsFolds.clear();
  if (!statsBusy && needsStatistics()) fetchStatistics();
  const controls = `<form id="stats-filter" class="stats-controls"><label>Projekt<select id="stats-project"><option value="">Alle Projekte</option>${S.projects.map(p => `<option value="${esc(p.id)}"${p.id === (statsFilterDraft.project ?? statsProject) ? ' selected' : ''}>${esc(p.name)}</option>`).join('')}</select></label>${statsTab === 'numbers' ? `<label>Von<input id="stats-from" type="date" value="${esc(statsFilterDraft.from ?? (statsFrom || statsData?.from || ''))}"></label><label>Bis<input id="stats-until" type="date" value="${esc(statsFilterDraft.until ?? (statsUntil || statsData?.until || S.today))}"></label>` : ''}<button type="submit" class="btn">Anzeigen</button></form>`;
  let body;
  if (statsTab === 'history') body = renderContentHistory();
  else if (!statsData) body = '<p class="muted">Lade Messstände …</p>';
  else if (statsData.error) body = `<p class="note">${esc(statsData.error)}</p>`;
  else body = `<section class="mod stat-total"><h2>${statsProject ? 'Projektumfang' : 'Alle Projekte im Überblick'}</h2><p><strong>${measured(statsData.summary?.source_code)}</strong> Quellcodezeilen · ${num(statsData.summary?.measured_projects)} von ${num(statsData.summary?.projects)} Projekten vollständig gemessen</p><p class="faint">Jeweils letzter gespeicherter Stand bis ${esc(fmtDate(statsData.until))}. Unterschiedliche Messzeitpunkte bleiben bei jedem Projekt sichtbar.</p>${languageBars(statsData.summary?.languages, statsData.summary?.source_code)}</section>${(statsData.projects || []).map(projectStatistics).join('')}`;
  return `<div class="statistics-page"><header class="ideas-heading"><h1>Statistik & Verlauf</h1><p>Wie sich deine Projekte entwickeln – mit gespeicherten Messständen und nachvollziehbaren Änderungen.</p></header><nav class="idea-filters" aria-label="Statistikansicht"><button class="idea-filter" type="button" data-stats-tab="numbers" aria-pressed="${statsTab === 'numbers'}">Zahlen & Entwicklung</button><button class="idea-filter" type="button" data-stats-tab="history" aria-pressed="${statsTab === 'history'}">Änderungsverlauf</button></nav>${controls}${statsTab === 'numbers' ? `<div class="stat-presets">${[[7, '7 Tage'], [30, '30 Tage'], [90, '90 Tage'], [365, '1 Jahr']].map(([days, title]) => `<button class="linkish" type="button" data-stats-range="${days}">${title}</button>`).join('')}</div>` : ''}${body}</div>`;
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
