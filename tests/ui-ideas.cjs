const test = require('node:test');
const assert = require('node:assert/strict');
const { app } = require('./ui-helpers.cjs');

const expose = `Object.assign(ui, { renderIdeas, ideaCollection, ideaBucket, setIdeaFilter, openIdeaResult, restoreIdeaProject, addIdea });`;
const ids = entries => Array.from(entries, entry => entry.s.id);

test('one idea collection orders all sources by first appearance and preserves remembered proposals', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.projects[0].suggestion_first_seen = { s1: '2026-10-03T12:00:00Z' };
  a.state.portfolio = {
    new_apps: [{ id: 'app', title: 'New application', pitch: 'App pitch' }],
    synergies: [{ id: 'link', title: 'Shared capability', projects: ['sample'], why: 'Reuse the same capability' }],
  };
  a.state.new_app_first_seen = { app: '2026-10-05T12:00:00Z' };
  a.state.synergy_first_seen = { link: '2026-10-04T12:00:00Z' };
  a.state.ideas = [{ id: 8, text: 'My own idea', added_at: '2026-10-06T12:00:00Z' }];
  a.state.feedback.saved = { value: 'up', title: 'Remembered proposal', project_id: 'sample', first_seen: '2026-10-01T12:00:00Z', item: { id: 'saved', kind: 'suggestion', title: 'Remembered proposal', why: 'Still useful' } };
  a.state.feedback.focus = { value: 'done', item: { kind: 'focus', title: 'Not an idea' } };
  await a.ui.load(false);
  assert.deepEqual(ids(a.ui.ideaCollection()), ['idea:8', 'app', 'link', 's1', 'saved']);
  a.ui.setIdeaFilter('all');
  const html = a.ui.renderIdeas();
  for (const label of ['Deine Idee', 'Neue App', 'Querverbindung', 'Aus dem Projekt', 'aus früherem Bericht gemerkt']) assert.ok(html.includes(label));
  assert.doesNotMatch(html, /Not an idea/);
  assert.equal((html.match(/data-idea-id="saved"/g) || []).length, 1);
  a.state.projects[0].card.suggestions.push({ id: 'saved', title: 'Rewritten old proposal' });
  a.state.projects[0].suggestion_first_seen.saved = '2026-10-01T12:00:00Z';
  await a.ui.load(false);
  assert.deepEqual(ids(a.ui.ideaCollection()), ['idea:8', 'app', 'link', 's1', 'saved']);
  assert.match(a.ui.renderIdeas(), /Rewritten old proposal/);
  assert.doesNotMatch(a.ui.renderIdeas(), /Remembered proposal/);
});

test('ratings, dated returns and explicit task state have separate reachable filters', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.projects[0].card.suggestions = ['fresh', 'good', 'later', 'returned', 'rejected', 'working', 'finished', 'cancelled'].map(id => ({ id, title: `Title ${id}` }));
  a.state.feedback = {
    good: { value: 'up' }, later: { value: 'later', snoozed_until: '2026-10-08' },
    returned: { value: 'later', snoozed_until: '2026-10-07' }, rejected: { value: 'down' },
    finished: { value: 'up' },
  };
  a.state.tasks = [
    { id: 1, source_id: 'working', status: 'review', title: 'Title working', projects: ['sample'], revision: 2 },
    { id: 2, source_id: 'finished', status: 'done', title: 'Title finished', projects: ['sample'], revision: 3 },
    { id: 3, source_id: 'cancelled', status: 'cancelled', title: 'Title cancelled', projects: ['sample'], revision: 2 },
  ];
  await a.ui.load(false);
  const buckets = Object.fromEntries(a.ui.ideaCollection().map(e => [e.s.id, a.ui.ideaBucket(e)]));
  assert.deepEqual(buckets, { fresh: 'new', good: 'up', later: 'later', returned: 'new', rejected: 'down', working: 'assigned', finished: 'done', cancelled: 'done' });
  let html = a.ui.renderIdeas();
  assert.match(html, /Title fresh/); assert.match(html, /Wieder da · aufgeschoben bis 07\.10\.2026/);
  assert.doesNotMatch(html, /Title later|Title finished|Title working|Title rejected/);
  for (const filter of ['up', 'later', 'assigned', 'done', 'down', 'all']) assert.match(html, new RegExp(`data-idea-filter="${filter}"`));
  a.ui.setIdeaFilter('assigned'); html = a.ui.renderIdeas();
  assert.match(html, /Title working/); assert.match(html, /Ergebnis bestätigen/);
  assert.doesNotMatch(html, /Title finished/);
  a.ui.setIdeaFilter('done'); html = a.ui.renderIdeas();
  assert.match(html, /Erledigt bestätigt/); assert.match(html, /Wieder öffnen/);
  assert.doesNotMatch(html, /data-task-assign="finished"/);
  a.ui.setIdeaFilter('down'); assert.match(a.ui.renderIdeas(), /Title rejected/);
});

test('an assigned idea survives source removal without collecting unrelated tasks', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.projects[0].card.suggestions = [];
  a.state.tasks = [
    { id: 3, source_id: 'old', status: 'blocked', title: 'Remember this idea', why: 'Actual assignment reason', evidence: 'Actual assignment evidence', done_when: 'Actual criterion', projects: ['sample'], source_snapshot: { kind: 'suggestion', id: 'old', title: 'Historical wording', why: 'Historical reason' } },
    { id: 4, source_id: 'focus', status: 'assigned', title: 'Focus task', projects: ['sample'], source_snapshot: { kind: 'focus', title: 'Focus task' } },
    { id: 5, source_id: 'other', status: 'assigned', title: 'Arbitrary task', projects: ['sample'] },
  ];
  await a.ui.load(false);
  assert.deepEqual(ids(a.ui.ideaCollection()), ['old']);
  a.ui.setIdeaFilter('assigned');
  assert.match(a.ui.renderIdeas(), /Remember this idea/);
  assert.match(a.ui.renderIdeas(), /Actual assignment reason/);
  assert.match(a.ui.renderIdeas(), /Actual assignment evidence/);
  assert.doesNotMatch(a.ui.renderIdeas(), /Historical wording|Historical reason/);
  assert.match(a.ui.renderIdeas(), /Wieder freigeben/);
  assert.doesNotMatch(a.ui.renderIdeas(), /Focus task|Arbitrary task/);
});

test('opened idea evidence and task results stay open across refresh and can close again', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.projects[0].card.suggestions[0].evidence = 'Concrete evidence';
  a.state.ideas = [{ id: 8, text: 'Own assigned idea', added_at: '2026-10-06' }];
  a.state.tasks = [
    { id: 11, source_id: 's1', status: 'review', title: 'Suggestion', projects: ['sample'], revision: 2, result: 'Original result' },
    { id: 12, source_id: 'idea:8', status: 'review', title: 'Own assigned idea', projects: [], revision: 2, result: 'Own result' },
  ];
  await a.ui.load(false);
  a.ui.setIdeaFilter('assigned');
  for (const fold of ['idea-task:s1:11', 'idea-task:idea:8:12', 'idea-evidence:s1']) {
    a.dispatch('toggle', { target: { dataset: { fold }, open: true, matches: () => false } });
  }
  a.state.tasks[0].result = 'Updated result';
  a.state.tasks[0].revision = 3;
  await a.ui.load(false);
  let html = a.ui.renderIdeas();
  assert.match(html, /Updated result/);
  for (const fold of ['idea-task:s1:11', 'idea-task:idea:8:12', 'idea-evidence:s1']) {
    assert.ok(html.includes(`data-fold="${fold}" open`));
    a.dispatch('toggle', { target: { dataset: { fold }, open: false, matches: () => false } });
  }
  await a.ui.load(false);
  html = a.ui.renderIdeas();
  assert.doesNotMatch(html, /data-fold="(?:idea-task|idea-evidence):[^"]+" open/);
});

test('filter changes and source rewrites preserve an open note and snooze input', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.feedback.s1 = { value: 'down', item: { kind: 'suggestion', id: 's1', title: 'Suggestion' }, project_id: 'sample' };
  await a.ui.load(false);
  a.ui.note('s1'); a.ui.draft('s1', 'A reason still being written');
  a.ui.setIdeaFilter('up');
  assert.match(a.ui.renderIdeas(), /Offene Eingabe/);
  assert.match(a.ui.renderIdeas(), /value="A reason still being written"/);
  await a.ui.load(false);
  assert.match(a.ui.renderIdeas(), /value="A reason still being written"/);
  a.ui.openSnooze('s1');
  a.dispatch('input', { target: { id: 'snooze-date', value: '2026-11-09', dataset: {}, matches: () => false } });
  a.ui.setIdeaFilter('all');
  a.state.projects[0].card.suggestions = [];
  a.state.feedback = {};
  await a.ui.load(false);
  const html = a.ui.renderIdeas();
  assert.match(html, /data-snooze-save/);
  assert.match(html, /value="2026-11-09"/);
});

test('own idea assessments remain advice and removed owner ideas do not return through feedback', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.ideas = [{ id: 7, text: 'My idea', added_at: '2026-10-06' }];
  a.state.portfolio = { idea_checks: [{ idea: 7, verdict: 'later', why: 'Agent recommends waiting', fits: ['sample'] }] };
  a.state.feedback['idea:8'] = { value: 'up', item: { kind: 'owner_idea', title: 'Removed idea', id: 'idea:8' } };
  await a.ui.load(false);
  const own = a.ui.ideaCollection().find(e => e.s.id === 'idea:7');
  assert.equal(a.ui.ideaBucket(own), 'new');
  assert.match(a.ui.renderIdeas(), /Agent-Einschätzung/);
  assert.match(a.ui.renderIdeas(), /data-item="idea:7"/);
  assert.doesNotMatch(a.ui.renderIdeas(), /Removed idea/);
  assert.equal(a.calls.filter(c => c.command === 'set_feedback').length, 0);
});

test('changing the rating of a remembered proposal keeps its source before the next refresh', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.projects[0].card.suggestions = [];
  a.state.feedback.old = { value: 'later', snoozed_until: '2026-10-09', project_id: 'sample', first_seen: '2026-09-10T12:00:00Z', item: { kind: 'suggestion', id: 'old', title: 'Saved old proposal', why: 'Useful' } };
  await a.ui.load(false);
  a.ui.setIdeaFilter('later');
  await a.ui.setFeedback({ dataset: { item: 'old' } }, 'up');
  a.ui.setIdeaFilter('up');
  assert.match(a.ui.renderIdeas(), /Saved old proposal/);
  const entry = a.ui.ideaCollection().find(e => e.s.id === 'old');
  assert.equal(entry.project, 'sample');
  assert.equal(entry.firstSeen, '2026-09-10T12:00:00Z');
  assert.equal(a.calls.find(c => c.command === 'set_feedback').args.value, 'up');
});

test('search opens the correct idea filter and highlights the stable source id', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'board';
  a.state.feedback.s1 = { value: 'down' };
  await a.ui.load(false);
  await a.ui.openIdeaResult('s1');
  const html = a.ui.renderIdeas();
  assert.match(html, /data-idea-filter="down" aria-pressed="true"/);
  assert.match(html, /class="idea search-hit" data-idea-id="s1"/);
  assert.equal(a.calls.find(c => c.command === 'set_pref').args.value, 'ideas');
});

test('remembered ideas from removed projects require restoration before a new handoff', async () => {
  const a = await app((command, args, state) => {
    if (command === 'restore_project') {
      const project = { id: args.id, name: 'Restored project', path: 'C:/restored', status: 'active' };
      state.projects.push(project);
      return { restored: true, project };
    }
    return {};
  }, expose);
  a.state.prefs.style = 'ideas';
  a.state.feedback.remembered = { value: 'up', project_id: 'removed', item: { id: 'remembered', title: 'Remembered work', kind: 'suggestion' } };
  await a.ui.load(false);
  a.ui.setIdeaFilter('up');
  let html = a.ui.renderIdeas();
  assert.match(html, /Projekt nicht mehr beobachtet: removed/);
  assert.match(html, /data-idea-restore-project="removed"/);
  assert.doesNotMatch(html, /data-task="remembered"/);
  assert.equal(a.calls.filter(c => c.command === 'restore_project').length, 0);
  await a.ui.restoreIdeaProject('removed');
  html = a.ui.renderIdeas();
  assert.match(html, /data-task="remembered"/);
  assert.match(html, /Restored project/);
  assert.doesNotMatch(html, /data-idea-restore-project/);
  assert.equal(a.calls.filter(c => c.command === 'restore_project').length, 1);
});

test('newly saved own ideas become visible from any collection filter and failed saves retain the draft', async () => {
  let fail = false;
  const a = await app((command, args, state) => {
    if (command === 'add_idea') {
      if (fail) throw new Error('Storage failed');
      state.ideas.push({ id: 21, text: args.text, added_at: '2026-10-07' });
      return { id: 21, created: true };
    }
    return {};
  }, expose);
  a.state.prefs.style = 'ideas';
  await a.ui.load(false);
  a.ui.setIdeaFilter('up');
  const input = text => {
    a.node('#idea-text').value = text;
    a.dispatch('input', { target: { id: 'idea-text', value: text, dataset: {}, matches: () => false } });
  };
  input('A newly captured idea');
  await a.ui.addIdea();
  let html = a.ui.renderIdeas();
  assert.match(html, /data-idea-filter="new" aria-pressed="true"/);
  assert.match(html, /class="idea search-hit" data-idea-id="idea:21"/);
  assert.match(html, /A newly captured idea/);
  a.ui.setIdeaFilter('later'); fail = true;
  input('Keep my unsaved idea');
  await a.ui.addIdea();
  html = a.ui.renderIdeas();
  assert.match(html, /value="Keep my unsaved idea"/);
  assert.match(html, /data-idea-filter="later" aria-pressed="true"/);
  assert.match(a.node('#toast').innerHTML, /Storage failed/);
});
