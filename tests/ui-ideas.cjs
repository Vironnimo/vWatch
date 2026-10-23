const test = require('node:test');
const assert = require('node:assert/strict');
const { app } = require('./ui-helpers.cjs');

const expose = `Object.assign(ui, { renderIdeas, ideaCollection, ideaBucket, setIdeaCategory, setIdeaFilter, openIdeaResult, restoreIdeaProject, addIdea, changeIdeaStatus, saveOwnerIdea, ideaEdit: (id, value) => ownerIdeaDrafts.set(id, value), ideaEditState: id => ownerIdeaDrafts.get(id) });`;
const ids = entries => Array.from(entries, entry => entry.s.id);
// Explicit service read models, not a copy of its Task selection algorithm.
const projection = (state, id, status, task, revision = 'idea-r1') => {
  state.idea_states[id] = { status, archived: false, revision, task };
};

test('direct progress and archive actions work without copying or rating, including the round', async () => {
  const a = await app((command, args, state) => {
    if (command !== 'set_idea_status') return {};
    assert.equal(args.revision, state.idea_states.s1.revision);
    const progress = args.status === 'in_progress' ? 'assigned' : args.status === 'implemented' ? 'done' : 'cancelled';
    if (args.status === 'archived' || args.status === 'restore') state.idea_states.s1.archived = args.status === 'archived';
    else {
      state.tasks = [{ id: 1, revision: 1, status: progress, source_id: 's1', title: 'Suggestion', projects: ['sample'] }];
      state.idea_states.s1.status = args.status === 'open' ? 'open' : args.status;
      state.idea_states.s1.task = state.tasks[0];
    }
    state.idea_states.s1.revision += 'x';
    return { changed: true, idea_status: state.idea_states.s1 };
  }, expose);
  a.state.prefs.style = 'ideas';
  a.state.idea_states = { s1: { status: 'open', archived: false, revision: 'r1' } };
  await a.ui.load(false);
  a.ui.startIdeaRound();
  assert.match(a.node('#round-root').innerHTML, /data-idea-status="in_progress"/);
  assert.match(a.node('#round-root').innerHTML, /data-idea-status="implemented"/);
  assert.match(a.node('#round-root').innerHTML, /data-idea-status="archived"/);
  await a.ui.changeIdeaStatus('s1', 'in_progress', 'r1');
  assert.equal(a.ui.ideaBucket(a.ui.ideaCollection()[0]), 'assigned');
  assert.doesNotMatch(a.ui.blockIdeas(), /Suggestion/);
  assert.match(a.node('#round-root').innerHTML, /Geschafft/);
  a.ui.setIdeaFilter('assigned');
  assert.match(a.ui.renderIdeas(), /In Umsetzung/);
  await a.ui.changeIdeaStatus('s1', 'implemented', 'r1x');
  a.ui.setIdeaFilter('done');
  assert.match(a.ui.renderIdeas(), /Suggestion/);
  await a.ui.changeIdeaStatus('s1', 'archived', 'r1xx');
  assert.equal(a.ui.ideaBucket(a.ui.ideaCollection()[0]), 'archived');
  assert.doesNotMatch(a.ui.renderIdeas(), /<div class="t">Suggestion/);
  a.ui.setIdeaFilter('archived');
  assert.match(a.ui.renderIdeas(), /Zurückholen/);
  await a.ui.changeIdeaStatus('s1', 'restore', 'r1xxx');
  assert.equal(a.ui.ideaBucket(a.ui.ideaCollection()[0]), 'done');
  await a.ui.changeIdeaStatus('s1', 'open', 'r1xxxx');
  assert.equal(a.ui.ideaBucket(a.ui.ideaCollection()[0]), 'new');
  assert.deepEqual(a.state.feedback, {});
  assert.equal(a.copied.length, 0);
  assert.equal(a.calls.filter(c => c.command === 'set_feedback' || c.command === 'create_task').length, 0);
});

test('status conflicts retain the actual state and expose the refreshed result', async () => {
  const a = await app((command, args, state) => {
    if (command === 'set_idea_status') {
      state.tasks[0].result = 'New agent report';
      throw { code: 'idea_changed' };
    }
  }, expose);
  a.state.idea_states = { s1: { archived: false, revision: 'old' } };
  a.state.tasks = [{ id: 1, status: 'review', source_id: 's1', projects: ['sample'], title: 'Suggestion' }];
  projection(a.state, 's1', 'in_progress', a.state.tasks[0], 'old');
  a.ui.setIdeaFilter('assigned');
  await a.ui.changeIdeaStatus('s1', 'implemented', 'old');
  assert.equal(a.ui.ideaBucket(a.ui.ideaCollection()[0]), 'assigned');
  assert.match(a.ui.renderIdeas(), /New agent report/);
});

test('an open snooze has exactly one editor inside its filter and stays pinned in other filters', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  await a.ui.load(false);
  a.ui.openSnooze('s1');
  let html = a.ui.renderIdeas();
  assert.equal((html.match(/data-snooze-save/g) || []).length, 1);
  assert.doesNotMatch(html, /class="idea-pending"/);
  a.ui.setIdeaFilter('up');
  html = a.ui.renderIdeas();
  assert.equal((html.match(/data-snooze-save/g) || []).length, 1);
  assert.match(html, /class="idea-pending"/);
});

test('idea progress displays the service-selected Task and keeps its revision with the idea snapshot', async () => {
  const a = await app(undefined, expose + 'Object.assign(ui, { openTaskPreview, closeTaskPreview, selectedTask: () => taskPreview?.task });');
  a.state.tasks = [
    { id: 3, status: 'done', source_id: 's1', title: 'Completed idea', projects: ['sample'] },
    { id: 1, status: 'review', source_id: 's1', title: 'Reviewed idea', projects: ['sample'] },
    { id: 2, status: 'blocked', source_id: 's1', title: 'Selected idea', projects: ['sample'] },
  ];
  projection(a.state, 's1', 'in_progress', { ...a.state.tasks[2], revision: 9, result: 'Coherent selected report' }, 'whole-source-revision');
  await a.ui.load(false);
  a.ui.setIdeaFilter('assigned');
  assert.match(a.ui.renderIdeas(), /data-task-id="2"/);
  assert.match(a.ui.renderIdeas(), /data-task-revision="9"/);
  assert.match(a.ui.renderIdeas(), /Coherent selected report/);
  assert.match(a.ui.renderIdeas(), /data-idea-revision="whole-source-revision"/);
  // The separately displayed Task list can come from another projection; it
  // must not overwrite the selected snapshot or its corresponding revision.
  a.state.tasks.push({ id: 99, status: 'review', source_id: 's1', result: 'Unrelated newer list entry' });
  await a.ui.load(false);
  assert.match(a.ui.renderIdeas(), /data-task-id="2"/);
  assert.doesNotMatch(a.ui.renderIdeas(), /Unrelated newer list entry/);
  a.ui.openTaskPreview('s1');
  assert.equal(a.ui.selectedTask().id, 2);
  assert.equal(a.ui.selectedTask().revision, 9);
  a.ui.closeTaskPreview();
  a.state.tasks = a.state.tasks.filter(t => t.id === 3);
  projection(a.state, 's1', 'implemented', a.state.tasks[0]);
  await a.ui.load(false);
  assert.equal(a.ui.ideaBucket(a.ui.ideaCollection()[0]), 'done');
});

test('one idea collection orders all sources by first appearance and preserves remembered proposals', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.projects[0].suggestion_first_seen = { s1: '2026-10-03T12:00:00Z' };
  a.state.portfolio = {
    new_apps: [{ id: 'app', title: 'New application', pitch: 'App pitch' }],
    synergies: [{ id: 'link', category: 'idea', title: 'Shared capability', projects: ['sample'], why: 'Reuse the same capability' }],
  };
  a.state.new_app_first_seen = { app: '2026-10-05T12:00:00Z' };
  a.state.synergy_first_seen = { link: '2026-10-04T12:00:00Z' };
  a.state.ideas = [{ id: 8, text: 'My own idea', added_at: '2026-10-06T12:00:00Z' }];
  a.state.feedback.saved = { value: 'up', title: 'Remembered proposal', project_id: 'sample', first_seen: '2026-10-01T12:00:00Z', item: { id: 'saved', kind: 'suggestion', category: 'idea', title: 'Remembered proposal', why: 'Still useful' } };
  a.state.feedback.focus = { value: 'done', item: { kind: 'focus', title: 'Not an idea' } };
  await a.ui.load(false);
  assert.deepEqual(ids(a.ui.ideaCollection()), ['idea:8', 'app', 'link', 's1', 'saved']);
  a.ui.setIdeaFilter('all');
  const html = a.ui.renderIdeas();
  for (const label of ['Deine Idee', 'Neue App', 'Querverbindung', 'Aus dem Projekt', 'aus früherem Bericht gemerkt']) assert.ok(html.includes(label));
  assert.doesNotMatch(html, /Not an idea/);
  assert.equal((html.match(/data-idea-id="saved"/g) || []).length, 1);
  a.state.projects[0].card.suggestions.push({ id: 'saved', category: 'idea', title: 'Rewritten old proposal' });
  a.state.projects[0].suggestion_first_seen.saved = '2026-10-01T12:00:00Z';
  await a.ui.load(false);
  assert.deepEqual(ids(a.ui.ideaCollection()), ['idea:8', 'app', 'link', 's1', 'saved']);
  assert.match(a.ui.renderIdeas(), /Rewritten old proposal/);
  assert.doesNotMatch(a.ui.renderIdeas(), /Remembered proposal/);
});

test('ratings, dated returns and explicit task state have separate reachable filters', async () => {
  const a = await app(undefined, expose + 'Object.assign(ui, { openTaskPreview, closeTaskPreview, selectedTask: () => taskPreview?.task });');
  a.state.prefs.style = 'ideas';
  a.state.projects[0].card.suggestions = ['fresh', 'good', 'later', 'returned', 'rejected', 'working', 'finished', 'cancelled'].map(id => ({ id, category: 'idea', title: `Title ${id}` }));
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
  projection(a.state, 'working', 'in_progress', a.state.tasks[0]);
  projection(a.state, 'finished', 'implemented', a.state.tasks[1]);
  projection(a.state, 'cancelled', 'open', a.state.tasks[2]);
  await a.ui.load(false);
  const buckets = Object.fromEntries(a.ui.ideaCollection().map(e => [e.s.id, a.ui.ideaBucket(e)]));
  assert.deepEqual(buckets, { fresh: 'new', good: 'up', later: 'later', returned: 'new', rejected: 'down', working: 'assigned', finished: 'done', cancelled: 'new' });
  let html = a.ui.renderIdeas();
  assert.match(html, /Title fresh/); assert.match(html, /Wieder da · aufgeschoben bis 07\.10\.2026/);
  assert.doesNotMatch(html, /Title later|Title finished|Title working|Title rejected/);
  a.ui.openTaskPreview('cancelled');
  assert.equal(a.ui.selectedTask().id, undefined, 'an ended assignment leaves a fresh handoff available');
  assert.equal(a.ui.selectedTask().source_id, 'cancelled');
  assert.equal(a.ui.selectedTask().title, 'Title cancelled');
  a.ui.closeTaskPreview();
  for (const filter of ['up', 'later', 'assigned', 'done', 'down', 'all']) assert.match(html, new RegExp(`data-idea-filter="${filter}"`));
  a.ui.setIdeaFilter('assigned'); html = a.ui.renderIdeas();
  assert.match(html, /Title working/); assert.match(html, /Ergebnis bestätigen/);
  assert.doesNotMatch(html, /Title finished/);
  a.ui.setIdeaFilter('done'); html = a.ui.renderIdeas();
  assert.match(html, /Erledigt bestätigt/); assert.match(html, /Wieder öffnen/);
  assert.doesNotMatch(html, /data-task-assign="finished"/);
  a.ui.setIdeaFilter('down'); assert.match(a.ui.renderIdeas(), /Title rejected/);
  a.state.ideas = [{ id: 8, text: 'Own idea after cancellation', added_at: '2026-10-06' }];
  const cancelled = { id: 8, source_id: 'idea:8', status: 'cancelled', title: 'Earlier own assignment', projects: [] };
  a.state.tasks.push(cancelled);
  projection(a.state, 'idea:8', 'open', cancelled);
  await a.ui.load(false); a.ui.setIdeaFilter('new');
  a.ui.openTaskPreview('idea:8');
  assert.equal(a.ui.selectedTask().id, undefined);
  assert.equal(a.ui.selectedTask().title, 'Own idea after cancellation');
  a.ui.closeTaskPreview();
});

test('an assigned idea survives source removal without collecting unrelated tasks', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.projects[0].card.suggestions = [];
  a.state.tasks = [
    { id: 3, source_id: 'old', status: 'blocked', title: 'Remember this idea', why: 'Actual assignment reason', evidence: 'Actual assignment evidence', done_when: 'Actual criterion', projects: ['sample'], source_snapshot: { kind: 'suggestion', category: 'idea', id: 'old', title: 'Historical wording', why: 'Historical reason' } },
    { id: 4, source_id: 'focus', status: 'assigned', title: 'Focus task', projects: ['sample'], source_snapshot: { kind: 'focus', title: 'Focus task' } },
    { id: 5, source_id: 'other', status: 'assigned', title: 'Arbitrary task', projects: ['sample'] },
  ];
  projection(a.state, 'old', 'in_progress', a.state.tasks[0]);
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
  projection(a.state, 's1', 'in_progress', a.state.tasks[0]);
  projection(a.state, 'idea:8', 'in_progress', a.state.tasks[1]);
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
  a.state.feedback.s1 = { value: 'down', item: { kind: 'suggestion', category: 'idea', id: 's1', title: 'Suggestion' }, project_id: 'sample' };
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
  a.state.feedback.old = { value: 'later', snoozed_until: '2026-10-09', project_id: 'sample', first_seen: '2026-09-10T12:00:00Z', item: { kind: 'suggestion', category: 'idea', id: 'old', title: 'Saved old proposal', why: 'Useful' } };
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
  a.state.feedback.remembered = { value: 'up', project_id: 'removed', item: { id: 'remembered', title: 'Remembered work', kind: 'suggestion', category: 'idea' } };
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

test('the idea round rates only new ideas one at a time, carries an optional reason and can be skipped', async () => {
  const a = await app((command, args) => command === 'set_feedback' ? { note: args.note } : {}, expose);
  a.state.prefs.style = 'ideas';
  a.state.projects[0].card.suggestions = ['one', 'two', 'three', 'four', 'rated'].map(id => ({ id, category: 'idea', title: `Title ${id}`, why: `Why ${id}` }));
  a.state.projects[0].suggestion_first_seen = { one: '2026-10-07T09:00:00Z', two: '2026-10-06T09:00:00Z', three: '2026-10-05T09:00:00Z', four: '2026-10-04T09:00:00Z' };
  a.state.feedback.rated = { value: 'down' };
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /data-iround-start>4 neue Ideen nacheinander bewerten/);
  assert.match(a.node('.switch [data-style="ideas"]').innerHTML, /class="nav-count"[^>]*>4</);
  a.ui.startIdeaRound();
  assert.match(a.node('#round-root').innerHTML, /Title one[^]*Why one/);
  assert.match(a.node('#round-root').innerHTML, /1 von 4/);
  a.dispatch('input', { target: { id: 'idea-round-note', value: 'Not for this project', dataset: { iroundNote: 'one' }, matches: () => false } });
  a.dispatch('keydown', { key: '3', preventDefault() {}, target: { id: '', dataset: {} } });
  await new Promise(resolve => setImmediate(resolve));
  const saved = a.calls.filter(c => c.command === 'set_feedback');
  assert.deepEqual(saved.map(c => [c.args.item_id, c.args.value, c.args.note]), [['one', 'down', 'Not for this project']]);
  assert.match(a.node('#round-root').innerHTML, /Title two/);
  await a.ui.ideaRoundStep('skip');
  assert.match(a.node('#round-root').innerHTML, /Title three/);
  // typing a digit into the reason never rates the idea
  a.dispatch('keydown', { key: '1', preventDefault() { throw new Error('rated while typing'); }, target: { id: 'idea-round-note', dataset: {} } });
  await a.ui.ideaRoundStep('up');
  await a.ui.ideaRoundStep('later');
  assert.match(a.node('#round-root').innerHTML, /Geschafft[^]*1 gut, 1 später, 1 Quatsch, 1 übersprungen/);
  assert.deepEqual(a.calls.filter(c => c.command === 'set_feedback').map(c => c.args.value), ['down', 'up', 'later']);
});


test('ideas and recommendations keep independent filters, rounds and search destinations', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.projects[0].card.suggestions.push(
    { id: 'release', type: 'next', title: 'Release veröffentlichen', why: 'Bereit' },
    { id: 'workflow', type: 'next', category: 'idea', title: 'Einfacherer Ablauf', why: 'Weniger Schritte' }
  );
  a.state.portfolio = { synergies: [{ id: 'cleanup', title: 'Gemeinsam aufräumen' }] };
  a.state.proposals = [{ scope: 'sample', kind: 'suggestion', first_seen: '2026-10-01', in_report: false,
    item: { id: 'retained', category: 'recommendation', title: 'Frühere Empfehlung' } }];
  await a.ui.load(false);
  assert.match(a.ui.renderIdeas(), /Suggestion/);
  assert.match(a.ui.renderIdeas(), /Einfacherer Ablauf/);
  assert.doesNotMatch(a.ui.renderIdeas(), /Release veröffentlichen|Gemeinsam aufräumen|Frühere Empfehlung/);
  assert.match(a.ui.renderIdeas(), /Ideen <span>2 neu/);
  assert.match(a.ui.renderIdeas(), /Empfehlungen <span>3 neu/);
  a.ui.startIdeaRound();
  assert.match(a.node('#round-root').innerHTML, /Ideen bewerten[^]*1 von 2/);
  a.ui.setIdeaCategory('recommendation');
  assert.equal(a.node('#round-root').innerHTML, '');
  assert.match(a.ui.renderIdeas(), /Release veröffentlichen/);
  assert.match(a.ui.renderIdeas(), /Gemeinsam aufräumen/);
  assert.match(a.ui.renderIdeas(), /Frühere Empfehlung/);
  assert.doesNotMatch(a.ui.renderIdeas(), /Eigene Idee festhalten|Einfacherer Ablauf/);
  a.ui.startIdeaRound();
  assert.match(a.node('#round-root').innerHTML, /Empfehlungen bewerten[^]*1 von 3/);
  a.ui.setIdeaCategory('idea');
  await a.ui.openIdeaResult('release');
  assert.match(a.ui.renderIdeas(), /data-idea-category="recommendation" aria-pressed="true"/);
  assert.match(a.ui.renderIdeas(), /data-idea-id="release"/);
  a.ui.openProject('sample');
  const page = a.node('#project-page').innerHTML;
  assert.match(page, />Empfehlungen</);
  assert.match(page, />Ideen</);
  assert.ok(page.indexOf('Release veröffentlichen') < page.indexOf('Einfacherer Ablauf'));
  assert.equal(a.calls.filter(c => c.command === 'set_feedback').length, 0);
});

test('reclassification keeps the rating, original date, assignment and open note', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'ideas';
  a.state.projects[0].suggestion_first_seen = { s1: '2026-09-01' };
  a.state.feedback.s1 = { value: 'up' };
  a.state.tasks = [{ id: 4, source_id: 's1', title: 'Suggestion', projects: ['sample'], status: 'assigned', revision: 1 }];
  projection(a.state, 's1', 'in_progress', a.state.tasks[0]);
  await a.ui.load(false);
  a.ui.setIdeaFilter('assigned');
  a.ui.note('s1');
  a.state.projects[0].card.suggestions[0].category = 'recommendation';
  await a.ui.load(false);
  assert.match(a.ui.renderIdeas(), /Offene Eingabe[^]*data-note-save="s1"/);
  a.ui.note(null);
  a.ui.setIdeaCategory('recommendation');
  assert.match(a.ui.renderIdeas(), /data-idea-id="s1"/);
  assert.match(a.ui.renderIdeas(), /datetime="2026-09-01"/);
  assert.match(a.ui.renderIdeas(), /data-task-copy="s1"|Auftrag ansehen/);
  assert.match(a.ui.renderIdeas(), /data-v="up" aria-pressed="true"/);
  assert.equal(a.calls.filter(c => c.command === 'set_feedback' || c.command === 'create_task').length, 0);
});

test('unrated retained agent proposals stay visible and own idea edits retain drafts on failure', async () => {
  let fail = true;
  const a = await app(async (command) => {
    if (command === 'update_idea') {
      if (fail) throw { message: 'Newer revision exists' };
      return { undo_token: 'opaque idea receipt / v2' };
    }
    return {};
  }, expose);
  a.state.proposals = [{ id: 'retained', scope: 'sample', kind: 'suggestion', first_seen: '2026-10-01', in_report: false, item: { id: 'retained', title: 'Still available' } }];
  await a.ui.load(false);
  assert.ok(a.ui.ideaCollection().some(e => e.s.id === 'retained' && e.retained));
  a.ui.ideaEdit(4, { text: 'Überarbeitete Idee', revision: 3 });
  await a.ui.saveOwnerIdea(4);
  const write = a.calls.find(c => c.command === 'update_idea');
  assert.equal(write.args.revision, 3); assert.equal(write.args.text, 'Überarbeitete Idee');
  assert.equal(a.ui.ideaEditState(4).text, 'Überarbeitete Idee');
  fail = false;
  await a.ui.saveOwnerIdea(4);
  assert.equal(a.ui.ideaEditState(4), undefined);
  a.node('#undo').onclick();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(a.calls.find(c => c.command === 'undo').args.token, 'opaque idea receipt / v2');
});
