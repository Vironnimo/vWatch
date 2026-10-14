const test = require('node:test');
const assert = require('node:assert/strict');
const { app, deferred } = require('./ui-helpers.cjs');
const expose = `Object.assign(ui, { renderStatistics, metricCurve, languageBars, projectStatistics, fetchStatistics, invalidateStatistics, historyDetail, saveOwnerIdea, ideaCollection, keepStatisticsDraft,
  stats: data => { statsData = data; }, history: (data, versions) => { statsTab = 'history'; statsHistory = data; statsVersions = versions; },
  detail: data => { statsDetail = data; }, ideaEdit: (id, value) => ownerIdeaDrafts.set(id, value), ideaEditState: id => ownerIdeaDrafts.get(id) });`;

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

test('unrated retained agent proposals stay visible and own idea edits retain drafts on failure', async () => {
  const a = await app(async (command) => { if (command === 'update_idea') throw { message: 'Newer revision exists' }; return {}; }, expose);
  a.state.proposals = [{ id: 'retained', scope: 'sample', kind: 'suggestion', first_seen: '2026-10-01', in_report: false, item: { id: 'retained', title: 'Still available' } }];
  await a.ui.load(false);
  assert.ok(a.ui.ideaCollection().some(e => e.s.id === 'retained' && e.retained));
  a.ui.ideaEdit(4, { text: 'Überarbeitete Idee', revision: 3 });
  await a.ui.saveOwnerIdea(4);
  const write = a.calls.find(c => c.command === 'update_idea');
  assert.equal(write.args.revision, 3); assert.equal(write.args.text, 'Überarbeitete Idee');
  assert.equal(a.ui.ideaEditState(4).text, 'Überarbeitete Idee');
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
  a.ui.invalidateStatistics(true);
  a.ui.stats({ projects: [], summary: {} });
  assert.match(a.ui.renderStatistics(), /value="2026-08-01"/);
  assert.match(a.ui.historyDetail(), /Vorher/);
  a.ui.invalidateStatistics();
  assert.equal(a.ui.historyDetail(), '');
});
