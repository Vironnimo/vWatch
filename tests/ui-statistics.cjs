const test = require('node:test');
const assert = require('node:assert/strict');
const { app, deferred } = require('./ui-helpers.cjs');
const expose = `Object.assign(ui, { renderStatistics, metricCurve, languageBars, projectStatistics, fetchStatistics, invalidateStatistics, resetStatistics, historyDetail, keepStatisticsDraft,
  stats: data => { statsData = data; }, openFold: id => openFolds.add(id), history: (data, versions) => { statsTab = 'history'; statsHistory = data; statsVersions = versions; },
  detail: data => { statsDetail = data; } });`;

test('statistics show missing values honestly and break curves at incomplete or incompatible observations', async () => {
  const a = await app(undefined, expose);
  const data = { from: '2026-10-01', until: '2026-10-07', summary: { source_code: null, measured_projects: 0, projects: 1 }, projects: [{ id: 'sample', name: 'Sample', samples: [], current: null, change: null, confirmed_completions: 0 }] };
  a.ui.stats(data);
  const html = a.ui.renderStatistics();
  assert.match(html, /unbekannt/); assert.match(html, /Noch kein belastbarer Größenvergleich/);
  assert.match(html, /Änderungsverlauf/); assert.doesNotMatch(html, /NaN|undefined|Infinity/);
  const samples = [
    { at: '2026-10-01T08:00:00Z', branch: 'main', path: '/repo', value: 10 },
    { at: '2026-10-02T08:00:00Z', branch: 'main', path: '/repo', value: null },
    { at: '2026-10-03T08:00:00Z', branch: 'main', path: '/repo', value: 20 },
    { at: '2026-10-04T08:00:00Z', branch: 'other', path: '/repo', value: 30 },
  ];
  const curve = a.ui.metricCurve(samples, s => s.value, 'Code', '#fff');
  assert.equal((curve.match(/<circle/g) || []).length, 3); assert.doesNotMatch(curve, / L[0-9]/);
  const bars = a.ui.languageBars([{ language: 'Python', category: 'source', code: 10 }, { language: 'Markdown', category: 'documentation', code: 100 }], 10);
  assert.match(bars, /Python/); assert.match(bars, /100 %/); assert.doesNotMatch(bars, /Markdown/);
});

test('saved history exposes older pages and escaped before/after comparisons without acknowledging content', async () => {
  const a = await app(undefined, expose);
  a.ui.history({ events: [{ id: 12, at: '2026-10-01T08:00:00Z', entity: 'card', entity_id: 'sample', title: '<script>no</script>', operation: 'saved', fields: ['headline'] }], next_before: 12 },
    { versions: [{ version: 1, created_at: '2026-10-01' }, { version: 2, created_at: '2026-10-02' }] });
  a.ui.detail({ from: 1, to: 2, changes: [{ field: 'headline', before: 'Alt', after: '<img src=x>' }] });
  const html = a.ui.renderStatistics();
  assert.match(html, /data-history-more="12"/); assert.match(html, /data-history-event="12"/);
  assert.match(html, /stats-version-from/); assert.match(html, /Vorher/); assert.match(html, /Nachher/);
  assert.match(html, /&lt;img/); assert.doesNotMatch(html, /<script>|<img src=x>/);
  assert.equal(a.calls.filter(c => c.command === 'mark_changes_read').length, 0);
});

test('a stale statistics response cannot replace a newer selected query', async () => {
  const pending = deferred(); let reads = 0;
  const a = await app(command => command === 'statistics' ? (++reads === 1 ? pending.promise : { from: '2026-10-01', until: '2026-10-07', projects: [], summary: { source_code: 22, measured_projects: 1, projects: 1 } }) : {}, expose);
  const first = a.ui.fetchStatistics(); a.ui.invalidateStatistics(); await a.ui.fetchStatistics();
  pending.resolve({ from: '2026-10-01', until: '2026-10-07', projects: [], summary: { source_code: 99999 } }); await first;
  const html = a.ui.renderStatistics(); assert.match(html, /22/); assert.doesNotMatch(html, /99\.999/);
});

test('live refresh retains unsubmitted filters and an immutable comparison', async () => {
  const a = await app(undefined, expose);
  a.ui.stats({ projects: [], summary: {} });
  a.ui.keepStatisticsDraft({ target: { id: 'stats-from', value: '2026-08-01' } });
  a.ui.detail({ from: 1, to: 2, changes: [{ field: 'headline', before: 'Alt', after: 'Neu' }] });
  a.ui.invalidateStatistics();
  a.ui.stats({ projects: [], summary: {} });
  assert.match(a.ui.renderStatistics(), /value="2026-08-01"/);
  assert.match(a.ui.historyDetail(), /Vorher/);
  a.ui.resetStatistics();
  assert.equal(a.ui.historyDetail(), '');
});

const settle = () => new Promise(resolve => setImmediate(resolve));

const historyClick = (a, dataset) => a.dispatch('click', { target: { closest: selector => selector.startsWith('[data-project-statistics],')
  ? { dataset, hasAttribute: name => name === 'data-stats-close-detail' && Object.hasOwn(dataset, 'statsCloseDetail') } : null } });

test('history details follow the last selection and a closed detail stays closed after pending reads', async () => {
  const first = deferred(), second = deferred(), last = deferred();
  const a = await app((command, args) => command === 'history_event' ? ({ 1: first, 2: second, 3: last })[args.id].promise : {}, expose);
  historyClick(a, { historyEvent: '1' });
  historyClick(a, { historyEvent: '2' });
  second.resolve({ event: { changes: [{ field: 'headline', before: 'Old', after: 'Latest selected event' }] } });
  await settle();
  first.resolve({ event: { changes: [{ field: 'headline', before: 'Old', after: 'Earlier selected event' }] } });
  await settle();
  assert.match(a.ui.historyDetail(), /Latest selected event/);
  assert.doesNotMatch(a.ui.historyDetail(), /Earlier selected event/);
  historyClick(a, { historyEvent: '3' });
  historyClick(a, { statsCloseDetail: '' });
  last.resolve({ event: { changes: [{ field: 'headline', before: 'Old', after: 'Closed event' }] } });
  await settle();
  assert.equal(a.ui.historyDetail(), '');
});

test('reloads keep the shown statistics and fetch again only after measured facts changed', async () => {
  let reads = 0;
  const a = await app(command => command === 'statistics' ? { from: '2026-10-01', until: '2026-10-07', projects: [], summary: { source_code: 1000 + ++reads, measured_projects: 1, projects: 1 } } : {}, expose);
  a.state.prefs.style = 'statistics'; a.state.projects[0].facts_collected_at = '2026-10-07T08:00:00Z';
  await a.ui.load(false); await settle();
  assert.equal(reads, 1); assert.match(a.node('#view').innerHTML, /1\.001/);
  await a.ui.load(false); await settle();
  assert.equal(reads, 1);
  a.state.projects[0].facts_collected_at = '2026-10-07T08:20:00Z';
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /1\.001/); assert.doesNotMatch(a.node('#view').innerHTML, /Lade Messstände/);
  await settle();
  assert.equal(reads, 2); assert.match(a.node('#view').innerHTML, /1\.002/);
});

test('observation rows list changes only, flat stretches draw no inner dots and closed sections hold no rows', async () => {
  const a = await app(undefined, expose);
  const sample = (day, code) => ({ at: `2026-10-0${day}T08:00:00Z`, ok: true, branch: 'main', path: '/repo', code: { ok: true, source_code: code, method: 'm' }, dirty: { files: 0 }, ahead: 0, ci: { state: 'green' } });
  const samples = [sample(1, 10), sample(2, 10), sample(3, 10), sample(4, 12), sample(5, 12)];
  const p = { id: 'sample', name: 'Sample', current: samples[4], samples, activity: { days: [{ date: '2026-10-01', commits: 1, added: 1, removed: 0 }] }, confirmed_completions: 0 };
  const closed = a.ui.projectStatistics(p);
  assert.match(closed, /Messwerte im Zeitraum \(2 Stände\)/); assert.doesNotMatch(closed, /<tbody>/);
  a.ui.openFold('stats:observations:sample');
  const open = a.ui.projectStatistics(p);
  assert.equal((open.match(/<tr>/g) || []).length, 3); assert.doesNotMatch(open, /NaN|undefined/);
  const curve = a.ui.metricCurve(samples, s => s.code.source_code, 'Code', '#fff');
  assert.equal((curve.match(/<circle/g) || []).length, 4); assert.equal((curve.match(/<path d="M[0-9.]+,/g) || []).length, 1);
});
