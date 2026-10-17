const test = require('node:test');
const assert = require('node:assert/strict');
const { app, fixture } = require('./ui-helpers.cjs');

const expose = `Object.assign(ui, { blockAttention, blockChanges, blockProjects, blockSafety, renderHandover, renderBoard,
  signals, openTaskResult, searchEntries, pickResult,
  search: query => { searchQ = query; return searchResults(); },
});`;
const facts = extra => ({ is_git: true, has_remote: true, remotes: [{ name: 'origin', url: 'https://example.test/repository' }], ahead: 0,
  dirty: { files: 0 }, branches: { unmerged: [] }, worktrees: { count: 0 }, ci: { state: 'green' }, ...extra });
const task = (id, status, projects = ['sample']) => ({ id, status, projects, source_id: `source:${id}`, title: `Task ${id}`, revision: 1 });
const countUnread = html => (html.match(/class="pill new">ungelesen</g) || []).length;
const fold = (a, id, open) => a.dispatch('toggle', { target: { dataset: { fold: id }, open } });

test('Today presents owner decisions and waiting work before suggestions and changes without project statistics', async () => {
  const a = await app(undefined, expose), p = a.state.projects[0];
  p.card.waiting_on_you = [{ id: 'q1', title: 'Choose the export format' }];
  p.goal = 'Publish the reviewed version'; p.goal_due = a.state.today;
  p.facts = facts({ weekly: [0, 2, 5] });
  a.state.tasks = [task(1, 'blocked'), task(2, 'review'), task(3, 'assigned')];
  a.state.portfolio = { greeting: 'A short morning overview', focus: [{ id: 'focus', project: 'sample', title: 'Continue the export work', why: 'The next useful step' }] };
  a.state.changes = { has_baseline: true, token: 'read-v2', entries: [{ project: 'sample', kind: 'new', title: 'The export now works' }] };
  await a.ui.load(false);
  const html = a.node('#view').innerHTML, focus = html.indexOf('data-mod="focus"'), changes = html.indexOf('data-mod="changes"');
  for (const text of ['Task 1', 'Task 2', 'Choose the export format', 'Publish the reviewed version']) {
    assert.ok(html.indexOf(text) >= 0 && html.indexOf(text) < focus);
  }
  assert.ok(focus < changes);
  assert.doesNotMatch(html, /class="tile"|class="spark"|class="prog"|data-mod="projects"|data-mod="pulse"|data-mod="kpis"/);
  assert.doesNotMatch(html, /Task 3/);
  // the header already navigates; Today holds no second set of area links
  assert.doesNotMatch(html, /data-nav=|overview-links/);
});

test('Projects preserve the project voice and factual headline and leave statistics in detail views', async () => {
  const a = await app(undefined, expose), p = a.state.projects[0];
  a.state.prefs.style = 'board';
  p.card.personality = 'I finally understand my exported files';
  p.card.headline = 'Import and export now share the same format';
  p.card.vision = { progress_pct: 87, progress_note: 'Most milestones are complete' };
  p.facts = facts({ weekly: [0, 2, 5] });
  await a.ui.load(false);
  const html = a.node('#view').innerHTML;
  assert.match(html, /I finally understand my exported files/);
  assert.match(html, /Import and export now share the same format/);
  assert.doesNotMatch(html, /class="spark"|class="prog"|87 %|data-mod="pulse"|data-mod="kpis"/);
  assert.match(html, /<details class="reference-fold" data-fold="history">/);
  assert.match(html, /<details class="reference-fold" data-fold="facts">/);
});

test('Today surfaces failed checks and missing folders while ordinary repository activity stays quiet', async () => {
  const a = await app(undefined, expose), p = a.state.projects[0];
  p.card.waiting_on_you = [];
  p.facts = facts({ has_remote: false, ahead: 40, dirty: { files: 700, lines_added: 200000 }, worktrees: { count: 6 } });
  await a.ui.load(false);
  assert.match(a.ui.blockAttention(), /Gerade wartet nichts auf dich/);
  const missing = structuredClone(p); missing.id = 'missing'; missing.name = 'Missing project'; missing.facts = { is_git: false, error: 'path_missing' };
  a.state.projects.push(missing);
  p.facts.ci = { state: 'red' };
  await a.ui.load(false);
  const html = a.ui.blockAttention();
  assert.match(html, /CI rot/);
  assert.match(html, /Ordner fehlt/);
  assert.match(html, /data-attention-project="sample" data-attention-tab="history" data-attention-anchor="facts"/);
  assert.match(html, /data-attention-project="missing" data-attention-tab="history" data-attention-anchor="facts"/);
  assert.doesNotMatch(html, /Gerade wartet nichts auf dich|700|40 lokale|6 Worktrees/);
});

test('Today names lasting growing risks and how long questions wait, and hides postponed questions', async () => {
  const a = await app(undefined, expose), p = a.state.projects[0];
  p.facts = facts({ ahead: 11 });
  p.risk_streaks = [{ kind: 'unpushed', since: '2026-01-01T08:00:00Z', days: 12, at_least: true, then: { commits: 7 }, now: { commits: 11 }, trend: 'growing' }];
  p.card.waiting_on_you = [{ id: 'q-old', title: 'Pick the mail provider', options: [{ title: 'Own server' }, { title: 'Relay' }], recommended: 2 },
    { id: 'q-later', title: 'Decide the release name' }];
  p.item_persistence = [{ kind: 'question', id: 'q-old', title: 'Pick the mail provider', days: 9, versions: 4 }];
  a.state.feedback = { 'q-later': { value: 'later', at: a.state.today, snoozed_until: '2999-01-01' } };
  await a.ui.load(false);
  const html = a.ui.blockAttention();
  assert.match(html, /Wächst/);
  assert.match(html, /11 Commits nicht gepusht, vorher 7/);
  assert.match(html, /seit mindestens 12 Tagen/);
  assert.match(html, /Pick the mail provider/);
  assert.match(html, /seit 9 Tagen offen/);
  assert.doesNotMatch(html, /Decide the release name/);
  // a short or shrinking streak is ordinary work and stays quiet
  p.risk_streaks[0].days = 3;
  await a.ui.load(false);
  assert.doesNotMatch(a.ui.blockAttention(), /Wächst/);
});

test('attention follows owner action rather than dirty file counts, local commits or missing remotes', async () => {
  const a = await app(undefined, expose), base = structuredClone(fixture().projects[0]);
  base.card.waiting_on_you = [];
  const routine = { ...structuredClone(base), id: 'routine', facts: facts({ has_remote: false, ahead: 900, dirty: { files: 600, lines_added: 200000, lines_removed: 80000, oldest_change: '2025-01-01' }, worktrees: { count: 20 } }) };
  const decision = { ...structuredClone(base), id: 'decision' }; decision.card.waiting_on_you = [{ id: 'only-question', title: 'Choose a supported format' }];
  const goal = { ...structuredClone(base), id: 'goal', goal: 'Publish', goal_due: a.state.today };
  const waiting = { ...structuredClone(base), id: 'waiting' };
  a.state.projects = [routine, decision, goal, waiting];
  a.state.tasks = [task(7, 'blocked', ['waiting'])];
  await a.ui.load(false);
  assert.equal(a.ui.risk(routine), 0);
  for (const p of [decision, goal, waiting]) assert.ok(a.ui.risk(p) > a.ui.risk(routine));
  const collision = { ...structuredClone(base), facts: facts({ collisions: [{ count: 1, files: ['src/main.rs'], a: { kind: 'main' }, b: { kind: 'worktree', name: 'feature' } }] }) };
  const before = a.ui.risk(collision);
  collision.warnings = [{ key: 'worktrees', acknowledged: true }];
  assert.ok(a.ui.risk(collision) < before);
  assert.match(a.ui.signals(collision), /Überschneidung/);
  const html = a.ui.blockProjects();
  assert.ok(html.indexOf('data-open="waiting"') < html.indexOf('data-open="routine"'));
});

test('unknown push comparison remains unknown and a configured remote never claims a backup', async () => {
  const a = await app(undefined, expose), p = a.state.projects[0];
  p.facts = facts({ ahead: null, upstream: null });
  await a.ui.load(false);
  const badges = a.ui.signals(p), table = a.ui.blockSafety();
  assert.match(badges, /Push-Stand unbekannt/);
  assert.match(badges, /Arbeitsstand sauber/);
  assert.doesNotMatch(badges, /gesichert|Backup|tag good/);
  assert.match(table, /Push-Stand unbekannt/);
  assert.match(table, /Remotes belegen keine Sicherung/);
  assert.doesNotMatch(table, />gesichert</);
});

test('project and activity unread markers follow the explicit reading state across app visits', async () => {
  const a = await app(undefined, expose), p = a.state.projects[0];
  p.card_updated_at = '2026-10-07T12:00:00Z';
  p.card.recent_changes = [{ date: '2026-10-07', title: 'First change' }, { date: '2026-10-06', title: 'Second change' }];
  p.card_unread = true; p.unread_recent_changes = [1];
  a.state.previous_visit = '2026-10-08T12:00:00Z';
  await a.ui.load(false);
  assert.equal(countUnread(a.ui.blockProjects()), 1);
  let recent = a.ui.blockRecent();
  assert.equal(countUnread(recent), 1);
  assert.match(recent, /Second change <span class="pill new">ungelesen</);
  assert.doesNotMatch(recent, /First change <span class="pill new">ungelesen</);
  p.card_unread = false; p.unread_recent_changes = [];
  a.state.previous_visit = '2025-01-01T12:00:00Z';
  await a.ui.load(true);
  assert.equal(countUnread(a.ui.blockProjects()), 0);
  assert.equal(countUnread(a.ui.blockRecent()), 0);
  assert.equal(a.calls.filter(c => c.command === 'mark_changes_read').length, 0);
});

test('unread handbook or voice edits keep the explicit read action even without digest entries', async () => {
  const a = await app(undefined, expose);
  a.state.projects[0].card_unread = true;
  a.state.changes = { has_baseline: true, token: 'semantic-reading-state', entries: [] };
  await a.ui.load(false);
  let html = a.ui.blockChanges();
  assert.match(html, /ungelesene Änderungen/);
  assert.match(html, /data-open="sample"/);
  assert.match(html, /data-changes-read="semantic-reading-state"/);
  assert.doesNotMatch(html, /Keine neuen Projektmeldungen/);
  a.state.projects[0].card_unread = false;
  await a.ui.load(false);
  html = a.ui.blockChanges();
  assert.match(html, /Keine neuen Projektmeldungen/);
  assert.doesNotMatch(html, /data-changes-read=/);
});

test('the idea collection and standalone task search stay reachable without registered projects', async () => {
  const a = await app((command, args, state) => {
    if (command === 'set_pref') state.prefs[args.key] = args.value;
    return {};
  }, expose);
  a.state.projects = [];
  a.state.prefs.style = 'ideas';
  a.state.ideas = [{ id: 11, text: 'A standalone reading journal', added_at: '2026-10-07' }];
  a.state.tasks = [{ ...task(8, 'done', []), title: 'Build the journal export', result: 'A standalone archive can be reopened' }];
  await a.ui.load(false);
  assert.equal(a.node('.switch').hidden, false);
  assert.match(a.node('#view').innerHTML, /A standalone reading journal/);
  assert.equal(a.ui.search('reading journal').find(r => r.ideaId).ideaId, 'idea:11');
  const results = a.ui.search('standalone archive');
  assert.equal(results[0].taskId, 8);
  const destination = a.node('.task-inspection [data-search-anchor="task:8"]');
  destination.classList = { add(value) { destination.highlight = value; } };
  destination.scrollIntoView = options => { destination.scrolled = options; };
  a.ui.pickResult({ dataset: { sres: '0' } });
  await new Promise(resolve => setImmediate(resolve));
  assert.match(a.node('#view').innerHTML, /task-inspection/);
  assert.match(a.node('#view').innerHTML, /A standalone archive can be reopened/);
  assert.equal(destination.highlight, 'search-hit');
  assert.equal(a.document.activeElement, destination);
  assert.deepEqual(a.calls.filter(c => c.command === 'set_pref').map(c => c.args.value), ['handover']);
  assert.equal(a.calls.filter(c => ['create_task', 'set_task_status'].includes(c.command)).length, 0);
});

test('Projects still provide assigned and completed task history after the last project is removed', async () => {
  const a = await app(undefined, expose);
  a.state.projects = [];
  a.state.prefs.style = 'board';
  a.state.tasks = [task(21, 'assigned', ['removed-project']), task(22, 'done', [])];
  await a.ui.load(false);
  const html = a.node('#view').innerHTML;
  assert.match(html, /data-search-anchor="task:21"/);
  assert.match(html, /data-search-anchor="task:22"/);
  assert.match(html, /abgeschlossene Aufträge/);
  assert.match(html, /data-task-status="assigned"/);
  assert.doesNotMatch(html, /Watchtower konnte seine Daten nicht laden/);
});

test('opened attention, digest, history and fact disclosures survive rendering and state refreshes', async () => {
  const a = await app(undefined, expose);
  a.state.tasks = [task(5, 'review')];
  a.state.changes = { has_baseline: true, token: 'fold-state', entries: [{ project: 'sample', kind: 'new', title: 'A useful change' }] };
  await a.ui.load(false);
  for (const id of ['attention:5', 'digest:sample', 'history', 'facts']) fold(a, id, true);
  a.ui.render();
  assert.match(a.node('#view').innerHTML, /data-fold="attention:5" open/);
  assert.match(a.node('#view').innerHTML, /data-fold="digest:sample" open/);
  a.state.prefs.style = 'board';
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /data-fold="history" open/);
  assert.match(a.node('#view').innerHTML, /data-fold="facts" open/);
  fold(a, 'history', false); a.ui.render();
  assert.match(a.node('#view').innerHTML, /data-fold="history">/);
  assert.match(a.node('#view').innerHTML, /data-fold="facts" open/);
  a.state.prefs.style = 'handover';
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /data-fold="attention:5" open/);
  assert.match(a.node('#view').innerHTML, /data-fold="digest:sample" open/);
});

test('the footer offers only collapsed sections in the current area without rewriting legacy preferences', async () => {
  const a = await app(undefined, expose);
  const legacy = ['pulse', 'kpis', 'cards', 'projects', 'focus', 'changes'];
  a.state.prefs.hidden_modules = [...legacy];
  a.state.prefs.style = 'board';
  await a.ui.load(false);
  let html = a.node('#foot').innerHTML;
  assert.match(html, /data-mod-toggle="projects"/);
  assert.doesNotMatch(html, /data-mod-toggle="(?:pulse|kpis|cards|focus|changes)"/);
  a.state.prefs.style = 'handover';
  await a.ui.load(false);
  html = a.node('#foot').innerHTML;
  assert.match(html, /data-mod-toggle="focus"/);
  assert.match(html, /data-mod-toggle="changes"/);
  assert.doesNotMatch(html, /data-mod-toggle="(?:pulse|kpis|cards|projects)"/);
  assert.deepEqual(a.state.prefs.hidden_modules, legacy);
  assert.equal(a.calls.filter(c => c.command === 'set_pref').length, 0);
});
