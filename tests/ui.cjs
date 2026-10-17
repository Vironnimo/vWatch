// Run with node --test tests/ui*.cjs.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { app, fixture, deferred } = require('./ui-helpers.cjs');

test('legacy effort estimates stay invisible while recommendations and task actions remain usable', async () => {
  const a = await app();
  const card = a.state.projects[0].card;
  card.reentry = { next_step: 'Continue the work', why: 'A concrete next step', done_when: 'The result works', effort: 'S' };
  card.suggestions[0] = { ...card.suggestions[0], effort: 'M', evidence: 'Observed need', done_when: 'Suggestion delivered' };
  a.state.ideas = [{ id: 7, text: 'Parked idea', added_at: '2026-10-07' }];
  a.state.portfolio = {
    focus: [{ id: 'f1', project: 'sample', title: 'Focus item', why: 'First priority', done_when: 'Focus delivered', effort: 'half-day' }],
    patterns: [{ id: 'p1', projects: ['sample'], title: 'Pattern item', why: 'Shared issue', effort: 'hour' }],
    synergies: [{ id: 'y1', category: 'idea', projects: ['sample'], title: 'Synergy item', why: 'Shared solution', effort: 'day' }],
    new_apps: [{ id: 'n1', title: 'New app item', pitch: 'Useful app', effort: 'week' }],
    idea_checks: [{ idea: 7, verdict: 'fold_in', fits: ['sample'], why: 'Fits the existing project', effort: 'L' }],
  };
  const noEstimates = markup => {
    assert.doesNotMatch(markup, /class="effort"/);
    assert.doesNotMatch(markup, />(?:half-day|hour|day|week|S|M|L)</);
  };
  for (const style of ['handover', 'board', 'ideas', 'statistics']) {
    a.state.prefs.style = style;
    await a.ui.load(false);
    const overview = a.node('#view').innerHTML;
    noEstimates(overview);
    if (style === 'handover') {
      assert.match(overview, /Focus item/);
      assert.match(overview, /data-task="f1"/);
      assert.match(overview, /data-focus-done="f1"/);
    }
    if (style === 'board') {
      for (const title of ['Pattern item', 'Synergy item']) assert.ok(overview.includes(title));
      for (const id of ['p1', 'y1']) assert.ok(overview.includes(`data-task="${id}"`));
      assert.match(overview, /3 Entscheidungen/);
    }
    if (style === 'ideas') {
      for (const title of ['New app item', 'Synergy item', 'Parked idea', 'Fits the existing project']) assert.ok(overview.includes(title));
      for (const id of ['n1', 'y1', 's1', 'idea:7']) assert.ok(overview.includes(`data-task="${id}"`));
    }
    a.ui.openProject('sample');
    const projectPage = a.node('#project-page').innerHTML;
    noEstimates(projectPage);
    assert.match(projectPage, /Continue the work/);
    assert.match(projectPage, /Observed need/);
    assert.match(projectPage, /data-task="reentry:sample"/);
    assert.match(projectPage, /data-task="s1"/);
    a.ui.closeProject();
  }
  await a.ui.copyTask('reentry:sample');
  assert.match(a.copied.at(-1), /Fertig, wenn: The result works/);
  await a.ui.copyTask('s1');
  assert.match(a.copied.at(-1), /Beleg: Observed need/);
  assert.match(a.copied.at(-1), /Fertig, wenn: Suggestion delivered/);
});

test('ideas show the newest proposals globally and expand without losing ratings or task actions', async () => {
  const a = await app();
  a.state.projects[0].card.suggestions = [
    { id: 'first', type: 'next', title: 'First priority', why: 'Highest value' },
    { id: 'feature', type: 'feature', title: 'Later feature', why: 'Useful next', done_when: 'Feature works' },
    { id: 'cleanup', type: 'cleanup', title: 'Third suggestion', why: 'Less urgent' },
    { id: 'extra1', type: 'feature', title: 'Older known idea' },
    { id: 'extra2', type: 'feature', title: 'Unknown date idea' },
    { id: 'rejected', type: 'feature', title: 'Rejected suggestion' },
    { id: 'later', type: 'feature', title: 'Postponed suggestion' },
  ];
  a.state.projects[0].suggestion_first_seen = {
    first: '2026-10-07T08:00:00Z', feature: '2026-10-08T09:00:00Z',
    cleanup: '2026-10-01T08:00:00Z', extra1: '2026-10-05T08:00:00Z', extra2: 'invalid',
  };
  a.state.projects[0].card_updated_at = '2026-10-09T12:00:00Z';
  a.state.projects.push({ ...structuredClone(a.state.projects[0]), id: 'other', name: 'Other',
    suggestion_first_seen: { other1: '2026-10-08T08:00:00Z', other2: '2026-10-06T08:00:00Z' },
    card: { ...structuredClone(a.state.projects[0].card), suggestions: [
      { id: 'other1', title: 'Other recent idea' }, { id: 'other2', title: 'Other older idea' },
    ] },
  });
  a.state.feedback = { feature: { value: 'up' }, rejected: { value: 'down' }, later: { value: 'later', snoozed_until: '2026-10-14' } };
  a.state.portfolio = { new_apps: [{ id: 'app', title: 'An app idea', pitch: 'Useful separately' }] };
  a.state.new_app_first_seen = { app: '2026-10-08T10:00:00Z' };
  await a.ui.load(false);
  let markup = a.ui.blockIdeas();
  assert.match(markup, /6 von 8 Vorschläge/);
  assert.match(markup, /Neueste zuerst/);
  assert.match(markup, /First priority/);
  assert.match(markup, /An app idea/);
  assert.match(markup, /Later feature/);
  assert.doesNotMatch(markup, /Third suggestion|Unknown date idea|Rejected suggestion|Postponed suggestion/);
  const expectedOrder = ['An app idea', 'Later feature', 'Other recent idea', 'First priority', 'Other older idea', 'Older known idea'];
  for (let i = 1; i < expectedOrder.length; i++) assert.ok(markup.indexOf(expectedOrder[i - 1]) < markup.indexOf(expectedOrder[i]));
  assert.match(markup, /datetime="2026-10-08T09:00:00Z"/);
  assert.ok(markup.indexOf('data-expand="ideas"') < markup.indexOf('id="ideas-list"'));
  const toggle = () => a.dispatch('click', { target: { closest: selector => selector === '[data-expand]' ? { dataset: { expand: 'ideas' } } : null } });
  toggle();
  markup = a.ui.blockIdeas();
  assert.match(markup, /8 Vorschläge/);
  assert.match(markup, /nur die 6 neuesten anzeigen/);
  assert.ok(markup.indexOf('Later feature') < markup.indexOf('First priority'));
  assert.ok(markup.indexOf('Later feature') < markup.indexOf('Third suggestion'));
  assert.ok(markup.indexOf('Third suggestion') < markup.indexOf('Unknown date idea'));
  assert.match(markup, /data-task="feature"/);
  assert.match(markup, /data-item="feature"[^]*?data-v="up" aria-pressed="true"/);
  assert.doesNotMatch(markup, /Rejected suggestion|Postponed suggestion/);
  await a.ui.copyTask('feature');
  assert.match(a.copied.at(-1), /Fertig, wenn: Feature works/);
  toggle();
  markup = a.ui.blockIdeas();
  assert.match(markup, /6 von 8 Vorschläge/);
  assert.match(markup, /data-task="first"/);
  assert.match(markup, /data-task="app"/);
  assert.match(markup, /data-mod-toggle="ideas" aria-expanded="true"/);
  assert.doesNotMatch(markup, /Third suggestion/);
  a.ui.note('extra2');
  a.ui.render();
  assert.match(a.ui.blockIdeas(), /data-note-save="extra2"/);
  a.ui.note(null);
  a.ui.openSnooze('cleanup');
  assert.match(a.ui.blockIdeas(), /Third suggestion/);
  assert.match(a.ui.blockIdeas(), /data-snooze-save/);
  a.ui.openProject('sample');
  const projectPage = a.node('#project-page').innerHTML;
  assert.ok(projectPage.indexOf('First priority') < projectPage.indexOf('Third suggestion'));
  assert.match(projectPage, />Empfehlungen</);
  assert.match(projectPage, />Ideen</);
  assert.ok(projectPage.indexOf('Third suggestion') < projectPage.indexOf('Unknown date idea'));
  assert.equal(a.calls.filter(c => c.command === 'set_feedback').length, 0);
});

test('idea timestamp ties and missing history preserve authored order without inventing dates', async () => {
  const a = await app();
  a.state.projects[0].card.suggestions = ['first', 'second', 'unknown1', 'unknown2'].map(id => ({ id, title: `Idea ${id}` }));
  a.state.projects[0].suggestion_first_seen = { first: '2026-10-08T08:00:00Z', second: '2026-10-08T08:00:00Z' };
  await a.ui.load(false);
  const markup = a.ui.blockIdeas();
  const expectedOrder = ['Idea first', 'Idea second', 'Idea unknown1', 'Idea unknown2'];
  for (let i = 1; i < expectedOrder.length; i++) assert.ok(markup.indexOf(expectedOrder[i - 1]) < markup.indexOf(expectedOrder[i]));
  assert.equal((markup.match(/<time /g) || []).length, 2);
  assert.doesNotMatch(markup, /data-expand="ideas"/);
});

test('previously hidden sections remain reachable and expand without changing other preferences', async () => {
  const a = await app((command, args, state) => {
    if (command === 'set_pref') state.prefs[args.key] = structuredClone(args.value);
    return {};
  });
  a.state.projects[0].card.recent_changes = [{ date: '2026-10-07', title: 'Latest event' }];
  a.state.prefs.hidden_modules = ['recent', 'pulse'];
  a.state.prefs.style = 'board';
  await a.ui.load(false);
  const markup = a.node('#view').innerHTML;
  assert.match(markup, /class="mod is-collapsed" data-mod="recent"/);
  assert.match(markup, /data-mod-toggle="recent" aria-expanded="false" aria-controls="mod-recent-body"/);
  assert.match(markup, /id="mod-recent-body" hidden/);
  assert.match(markup, /Was passiert ist ausklappen/);
  assert.doesNotMatch(markup, /data-hide=/);
  await a.ui.toggleModule('recent');
  assert.equal(a.state.prefs.hidden_modules.join(','), 'pulse');
  assert.equal(a.state.prefs.style, 'board');
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /data-mod-toggle="recent" aria-expanded="true"/);
  assert.doesNotMatch(a.node('#view').innerHTML, /id="mod-recent-body" hidden/);
  await a.ui.toggleModule('recent');
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /id="mod-recent-body" hidden/);
  assert.equal(a.calls.filter(c => c.command === 'set_pref').length, 2);
  assert.equal(a.calls.filter(c => c.command === 'mark_changes_read').length, 0);
});

test('a failed collapse restores the visible section and reports the unsaved preference', async () => {
  const a = await app(command => {
    if (command === 'set_pref') throw new Error('database busy');
    return {};
  });
  a.state.projects[0].card.recent_changes = [{ date: '2026-10-07', title: 'Still visible' }];
  a.state.prefs.style = 'board';
  await a.ui.load(false);
  await a.ui.toggleModule('recent');
  assert.match(a.node('#view').innerHTML, /data-mod-toggle="recent" aria-expanded="true"/);
  assert.doesNotMatch(a.node('#view').innerHTML, /id="mod-recent-body" hidden/);
  assert.match(a.node('#toast').innerHTML, /Einstellung nicht gespeichert/);
});

test('recent list expands and reduces at the top without collapsing its section', async () => {
  const a = await app();
  a.state.projects[0].card.recent_changes = Array.from({ length: 12 }, (_, i) => ({ date: '2026-10-07', title: `Event ${i}` }));
  await a.ui.load(false);
  let markup = a.ui.blockRecent();
  assert.ok(markup.indexOf('data-recent-all') < markup.indexOf('class="events"'));
  assert.match(markup, /10 neueste von 12/);
  assert.doesNotMatch(markup, /Event 11/);
  const click = () => a.dispatch('click', { target: { closest: selector => selector === '[data-recent-all]' ? {} : null } });
  click();
  markup = a.ui.blockRecent();
  assert.match(markup, /Event 11/);
  assert.match(markup, /nur die neuesten 10 anzeigen/);
  assert.match(markup, /data-mod-toggle="recent" aria-expanded="true"/);
  click();
  assert.doesNotMatch(a.ui.blockRecent(), /Event 11/);
  assert.equal(a.calls.filter(c => c.command === 'set_pref').length, 0);
});

test('a pending decision can only be submitted once and advances one question', async () => {
  const pending = deferred();
  const a = await app(command => command === 'set_feedback' ? pending.promise : {});
  a.ui.startRound();
  const first = a.ui.roundStep('answered', 'First');
  const second = a.ui.roundStep('answered', 'Second');
  pending.resolve({});
  await Promise.all([first, second]);
  assert.equal(a.calls.filter(c => c.command === 'set_feedback').length, 1);
  assert.match(a.node('#round-root').innerHTML, /Frage 2 von 3/);
});

test('closing a decision round while saving does not reopen it or reject', async () => {
  const pending = deferred();
  const a = await app(command => command === 'set_feedback' ? pending.promise : {});
  a.ui.startRound();
  const saving = a.ui.roundStep('answered', 'First');
  a.ui.closeRound();
  pending.resolve({});
  await saving;
  assert.equal(a.node('#round-root').innerHTML, '');
});

test('numeric free text in a decision answer is not an option shortcut', async () => {
  const a = await app();
  a.ui.startRound();
  let prevented = false;
  a.dispatch('keydown', { target: { id: 'round-answer', value: '' }, key: '1',
    preventDefault() { prevented = true; } });
  assert.equal(prevented, false);
  assert.equal(a.calls.filter(c => c.command === 'set_feedback').length, 0);
});

test('a refreshed card updates the active round and keeps its answer draft', async () => {
  const a = await app();
  a.ui.startRound();
  a.ui.draft('q1', 'My answer');
  a.state.projects[0].card.waiting_on_you[0].title = 'Updated question';
  await a.ui.load(false);
  assert.match(a.node('#round-root').innerHTML, /Updated question/);
  assert.match(a.node('#round-root').innerHTML, /value="My answer"/);
  a.state.projects[0].card.waiting_on_you.shift();
  await a.ui.load(false);
  assert.match(a.node('#round-root').innerHTML, /Question 2/);
});

test('later and down ratings leave the optional explanation visible in the ideas collection', async () => {
  for (const value of ['later', 'down']) {
    const a = await app();
    a.state.prefs.style = 'ideas';
    await a.ui.load(false);
    await a.ui.setFeedback({ dataset: { item: 's1' } }, value);
    if (value === 'later') await a.ui.saveSnooze('2026-10-14');
    assert.match(a.node('#view').innerHTML, /data-note-save="s1"/);
    a.ui.draft('s1', 'After the release');
    a.state.feedback.s1 = { value, at: new Date().toISOString() };
    await a.ui.load(false);
    assert.match(a.node('#view').innerHTML, /value="After the release"/);
  }
});

test('a failed feedback note can be retried without losing the text', async () => {
  let attempts = 0;
  const a = await app(command => {
    if (command === 'set_feedback' && ++attempts === 2) throw new Error('busy');
    return {};
  });
  await a.ui.setFeedback({ dataset: { item: 's1' } }, 'down');
  a.ui.draft('s1', 'Not useful');
  await a.ui.saveNote('s1');
  await a.ui.saveNote('s1');
  const saves = a.calls.filter(c => c.command === 'set_feedback' && c.args.note);
  assert.equal(saves.length, 2);
  assert.equal(saves[1].args.note, 'Not useful');
});

test('search highlights text without rewriting mark tags or HTML entities', async () => {
  const a = await app();
  assert.equal(a.ui.highlight('Markdown & API', 'mark a'), '<mark>Mark</mark>down &amp; <mark>A</mark>PI');
  assert.equal(a.ui.highlight('<script> & x', 'script & x'), '&lt;<mark>script</mark>&gt; <mark>&amp;</mark> <mark>x</mark>');
  assert.equal(a.ui.highlight('März', 'marz'), '<mark>März</mark>');
});

test('refresh preserves unsent idea and project text', async () => {
  const a = await app();
  a.state.prefs.style = 'ideas';
  await a.ui.load(false);
  const input = (id, value) => a.dispatch('input', {
    target: { id, value, matches: () => false },
  });
  input('idea-text', 'A useful new app');
  a.ui.openProject('sample');
  a.dispatch('click', { target: { closest: selector => selector === '[data-project-intent-edit]' ? {} : null } });
  input('goal-text', 'Ship next month');
  input('status-note', 'Only fix bugs');
  await a.ui.load(false);
  assert.doesNotMatch(a.node('#project-page').innerHTML, /id="project-owner-fields" hidden/);
  assert.match(a.node('#project-page').innerHTML, /value="Ship next month"/);
  assert.match(a.node('#project-page').innerHTML, /value="Only fix bugs"/);
  a.dispatch('click', { target: { closest: selector => selector === '.switch button' ? { dataset: { style: 'ideas' } } : null } });
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /value="A useful new app"/);
});

function editProjectField(a, field, value) {
  a.dispatch('input', { target: { value, id: `project-${field}-sample`,
    dataset: { projectEditField: field, project: 'sample' }, matches: () => false } });
}

test('editing a name and folder saves once under the existing project id', async () => {
  const pending = deferred();
  const a = await app(async (command, args, state) => {
    if (command === 'update_project') {
      await pending.promise;
      Object.assign(state.projects[0], args);
      return state.projects[0];
    }
    return {};
  });
  await a.ui.openPanel();
  assert.match(a.node('#panel-root').innerHTML, /data-project-edit="sample"/);
  a.ui.editProject('sample');
  editProjectField(a, 'name', 'Renamed');
  editProjectField(a, 'path', 'C:/renamed');
  const saving = a.ui.saveProjectEdit('sample');
  await a.ui.saveProjectEdit('sample');
  a.ui.cancelProjectEdit('sample');
  assert.match(a.node('#panel-root').innerHTML, /Speichert …/);
  assert.match(a.node('#panel-root').innerHTML, /data-project-save="sample" disabled/);
  pending.resolve();
  await saving;
  const saves = a.calls.filter(c => c.command === 'update_project');
  assert.equal(saves.length, 1);
  assert.deepEqual(JSON.parse(JSON.stringify(saves[0].args)), { id: 'sample', name: 'Renamed', path: 'C:/renamed' });
  assert.equal(a.state.projects[0].card.headline, 'Ready');
  assert.match(a.node('#panel-root').innerHTML, /<b>Renamed<\/b>/);
  assert.doesNotMatch(a.node('#panel-root').innerHTML, /data-project-save=/);
  assert.equal(a.calls.filter(c => ['add_projects', 'remove_project'].includes(c.command)).length, 0);
});

test('removing projects and ideas retains their data and offers targeted undo in the window', async () => {
  let project, idea;
  const a = await app((command, args, state) => {
    if (command === 'remove_project') {
      project = state.projects.find(p => p.id === args.id);
      state.projects = state.projects.filter(p => p.id !== args.id);
    }
    if (command === 'restore_project') state.projects.push(project);
    if (command === 'remove_idea') {
      idea = state.ideas.find(i => i.id === args.id);
      state.ideas = state.ideas.filter(i => i.id !== args.id);
    }
    if (command === 'restore_idea') state.ideas.push(idea);
    return { removed: args?.id };
  });
  a.state.ideas = [{ id: 17, text: 'A useful idea', added_at: '2026-10-07' }];
  a.state.prefs.style = 'ideas';
  await a.ui.load(false);
  const originalProject = structuredClone(a.state.projects[0]);
  const originalIdea = structuredClone(a.state.ideas[0]);
  await a.ui.openPanel();
  a.dispatch('click', { target: { closest: selector => selector === '[data-remove-ask]' ? { dataset: { removeAsk: 'sample' } } : null } });
  const confirmation = a.node('#panel-root').innerHTML;
  assert.match(confirmation, /Karten, Antworten und Verlauf bleiben gespeichert/);
  assert.match(confirmation, /data-remove="sample"/);
  assert.match(confirmation, /data-remove-cancel/);
  assert.doesNotMatch(confirmation, /werden gelöscht|verloren|watchtower projects restore/);
  await a.ui.removeProject('sample');
  assert.equal(a.state.projects.length, 0);
  assert.match(a.node('#toast').innerHTML, /Karten und Verlauf bleiben gespeichert/);
  assert.match(a.node('#toast').innerHTML, /Rückgängig/);
  assert.doesNotMatch(a.node('#toast').innerHTML, /watchtower|CLI/);
  a.node('#undo').onclick();
  await new Promise(setImmediate);
  assert.equal(a.calls.find(c => c.command === 'restore_project').args.id, 'sample');
  assert.deepEqual(a.state.projects[0], originalProject);
  assert.match(a.node('#panel-root').innerHTML, /Sample/);
  await a.ui.removeIdea(17);
  assert.equal(a.state.ideas.length, 0);
  assert.match(a.node('#toast').innerHTML, /bleibt gespeichert/);
  assert.match(a.node('#toast').innerHTML, /Rückgängig/);
  assert.doesNotMatch(a.node('#toast').innerHTML, /watchtower|CLI/);
  a.node('#undo').onclick();
  await new Promise(setImmediate);
  assert.equal(a.calls.find(c => c.command === 'restore_idea').args.id, 17);
  assert.deepEqual(a.state.ideas[0], originalIdea);
  assert.match(a.node('#view').innerHTML, /A useful idea/);
  assert.equal(a.calls.filter(c => c.command === 'remove_project').length, 1);
  assert.equal(a.calls.filter(c => c.command === 'remove_idea').length, 1);
});

test('project edits retain drafts, focus and cursor across refresh and panel reopening', async () => {
  const a = await app();
  await a.ui.openPanel();
  a.ui.editProject('sample');
  editProjectField(a, 'name', 'My renamed project');
  editProjectField(a, 'path', 'C:/moved');
  // The old input is replaced by render; focus and selection go to the new node.
  a.document.activeElement = { tagName: 'INPUT', id: 'project-name-sample', selectionStart: 3, selectionEnd: 10 };
  await a.ui.load(false);
  assert.equal(a.document.activeElement, a.node('#project-name-sample'));
  assert.equal(a.node('#project-name-sample').selectionStart, 3);
  assert.equal(a.node('#project-name-sample').selectionEnd, 10);
  assert.match(a.node('#panel-root').innerHTML, /value="My renamed project"/);
  assert.match(a.node('#panel-root').innerHTML, /value="C:\/moved"/);
  a.ui.closePanel();
  await a.ui.openPanel();
  assert.match(a.node('#panel-root').innerHTML, /value="My renamed project"/);
  a.ui.cancelProjectEdit('sample');
  a.ui.editProject('sample');
  assert.match(a.node('#panel-root').innerHTML, /value="Sample"/);
  assert.equal(a.calls.filter(c => c.command === 'update_project').length, 0);
});

test('failed project edits retain both fields and can be retried without sending unchanged fields', async () => {
  let attempts = 0;
  const a = await app((command, args, state) => {
    if (command === 'update_project') {
      if (++attempts === 1) throw { message: 'Folder is already registered' };
      Object.assign(state.projects[0], args);
    }
    return {};
  });
  await a.ui.openPanel();
  a.ui.editProject('sample');
  editProjectField(a, 'name', 'Renamed');
  editProjectField(a, 'path', 'C:/other');
  await a.ui.saveProjectEdit('sample');
  assert.match(a.node('#panel-root').innerHTML, /role="alert">Nicht gespeichert: Folder is already registered/);
  assert.match(a.node('#panel-root').innerHTML, /value="Renamed"/);
  assert.match(a.node('#panel-root').innerHTML, /value="C:\/other"/);
  assert.doesNotMatch(a.node('#panel-root').innerHTML, /data-project-save="sample" disabled/);
  editProjectField(a, 'path', 'C:/sample');
  await a.ui.saveProjectEdit('sample');
  assert.deepEqual(JSON.parse(JSON.stringify(a.calls.filter(c => c.command === 'update_project')[1].args)), { id: 'sample', name: 'Renamed' });
});

test('folder selection keeps the name and only changes the draft after a choice', async () => {
  const pending = deferred();
  let picks = 0;
  const a = await app(command => command === 'pick_folder' ? (++picks === 1 ? pending.promise : 'C:/new-place') : {});
  await a.ui.openPanel();
  a.ui.editProject('sample');
  editProjectField(a, 'name', 'My name');
  const picking = a.ui.pickProjectFolder('sample');
  await a.ui.pickProjectFolder('sample');
  await a.ui.saveProjectEdit('sample');
  assert.equal(picks, 1);
  pending.resolve(null);
  await picking;
  assert.match(a.node('#panel-root').innerHTML, /value="C:\/sample"/);
  await a.ui.pickProjectFolder('sample');
  assert.match(a.node('#panel-root').innerHTML, /value="C:\/new-place"/);
  assert.match(a.node('#panel-root').innerHTML, /value="My name"/);
  assert.equal(a.calls.filter(c => c.command === 'update_project').length, 0);
});

test('a saved project with failed fact collection is reported as saved', async () => {
  const a = await app((command, args, state) => {
    if (command === 'update_project') {
      Object.assign(state.projects[0], args);
      return { project: state.projects[0], changed: true, facts_refresh: { ok: false, error: { message: 'Git unavailable' } } };
    }
    return {};
  });
  await a.ui.openPanel();
  a.ui.editProject('sample');
  editProjectField(a, 'path', 'C:/moved');
  await a.ui.saveProjectEdit('sample');
  assert.match(a.node('#toast').innerHTML, /Projekt geändert. Fakten konnten noch nicht neu gemessen werden/);
  assert.doesNotMatch(a.node('#panel-root').innerHTML, /data-project-save=/);
  assert.match(a.node('#panel-root').innerHTML, /C:\/moved/);
});

test('a copied task labels today as export date instead of evidence date', async () => {
  const a = await app();
  const task = a.ui.taskPrompt({ projects: ['sample'], title: 'Old finding' });
  assert.match(task, /kopiert am 07\.10\.2026/);
  assert.doesNotMatch(task, /Stand 07\.10\.2026/);
});

test('later waits for an explicit future calendar date and preserves its note', async () => {
  const a = await app();
  a.state.prefs.style = 'ideas';
  a.state.feedback.s1 = { value: 'up', note: 'Keep this reason', at: '2026-10-06T12:00:00Z' };
  await a.ui.load(false);
  await a.ui.setFeedback({ dataset: { item: 's1' } }, 'later');
  assert.equal(a.calls.filter(c => c.command === 'set_feedback').length, 0);
  assert.match(a.node('#view').innerHTML, /data-snooze-days="1"/);
  await a.ui.saveSnooze('2026-10-07');
  assert.equal(a.calls.filter(c => c.command === 'set_feedback').length, 0);
  await a.ui.saveSnooze('2026-10-14');
  const save = a.calls.find(c => c.command === 'set_feedback');
  assert.equal(save.args.until, '2026-10-14');
  assert.equal(save.args.note, null);
  assert.match(a.node('#view').innerHTML, /Wieder am 14\.10\.2026/);
});

test('snoozes expire on the stored local date and undo uses the stored receipt', async () => {
  const old = { value: 'later', note: 'Next week', at: '2026-10-01T23:00:00Z', snoozed_until: '2026-10-07' };
  const token = '0123456789abcdef0123456789abcdef';
  const a = await app((command, args, state) => {
    if (command === 'set_feedback') {
      state.feedback.s1 = { value: args.value, note: args.note, snoozed_until: args.until };
      return { undo: `watchtower undo ${token}` };
    }
    if (command === 'undo') { state.feedback.s1 = old; return { restored: true }; }
    return {};
  });
  a.state.feedback.s1 = old;
  await a.ui.load(false);
  assert.equal(a.ui.rated('s1'), null);
  a.ui.openSnooze('s1');
  await a.ui.saveSnooze('2026-10-14');
  a.node('#undo').onclick();
  await new Promise(setImmediate);
  const undo = a.calls.find(c => c.command === 'undo');
  assert.equal(undo.args.token, token);
  assert.equal(a.calls.filter(c => c.command === 'restore_feedback').length, 0);
  assert.equal(a.ui.rated('s1'), null);
  assert.deepEqual(a.state.feedback.s1, old);
});

test('feedback undo keeps and shows a newer owner change when the receipt conflicts', async () => {
  const a = await app((command, args, state) => {
    if (command === 'set_feedback') return { undo: 'watchtower undo 0123456789abcdef0123456789abcdef' };
    if (command === 'undo') throw { code: 'undo_conflict', message: 'A newer reply is saved. Nothing was changed.' };
    return {};
  });
  await a.ui.setFeedback({ dataset: { item: 's1' } }, 'down');
  a.state.feedback.s1 = { value: 'up', note: 'The newer decision' };
  a.node('#undo').onclick();
  await new Promise(setImmediate);
  assert.equal(a.calls.filter(c => c.command === 'undo').length, 1);
  assert.equal(a.calls.filter(c => c.command === 'restore_feedback').length, 0);
  assert.equal(a.ui.rated('s1'), 'up');
  assert.match(a.node('#toast').innerHTML, /newer reply|Nothing was changed/);
});

test('copying never assigns; assignment is explicit and copied tracked tasks request a report', async () => {
  const a = await app((command, args, state) => {
    if (command === 'create_task') {
      const task = { ...args.payload, id: 12, status: 'assigned', created_at: '2026-10-07T12:00:00Z' };
      state.tasks = [task];
      return { created: true, task };
    }
    return {};
  });
  a.state.prefs.style = 'ideas';
  await a.ui.load(false);
  await a.ui.copyTask('s1');
  assert.equal(a.copied.length, 1);
  assert.equal(a.calls.filter(c => c.command === 'create_task').length, 0);
  await a.ui.assignTask('s1');
  assert.equal(a.calls.filter(c => c.command === 'create_task').length, 1);
  assert.equal(a.state.tasks[0].source_id, 's1');
  await a.ui.assignTask('s1');
  assert.equal(a.calls.filter(c => c.command === 'create_task').length, 1);
  await a.ui.copyTask('s1');
  assert.match(a.copied.at(-1), /watchtower tasks report 12 --status done/);
});

test('tracked results await owner confirmation and survive replacement of the source card', async () => {
  const a = await app((command, args, state) => {
    if (command === 'set_task_status') state.tasks[0].status = args.status;
    return {};
  });
  a.state.tasks = [{ id: 4, projects: ['sample'], source_id: 's1', title: 'Original request', status: 'review',
    result: 'Implemented', result_evidence: 'cargo test passed', created_at: '2026-10-07T12:00:00Z',
    history: [{ actor: 'agent', status: 'review', at: '2026-10-07T12:00:00Z', result: 'Implemented', evidence: 'cargo test passed' }] }];
  a.state.projects[0].card.suggestions = [];
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /Original request/);
  assert.match(a.node('#view').innerHTML, /cargo test passed/);
  assert.match(a.node('#view').innerHTML, /data-task-status="done"/);
  await a.ui.changeTaskStatus(4, 'done');
  a.ui.openProject('sample');
  assert.match(a.node('#project-page').innerHTML, /abgeschlossene Aufträge/);
  assert.match(a.node('#project-page').innerHTML, /Original request/);
  assert.match(a.node('#project-page').innerHTML, /Wieder öffnen/);
  await a.ui.changeTaskStatus(4, 'assigned', 1);
  assert.equal(a.state.tasks[0].status, 'assigned');
});

test('tracked task copies retain the original project after it leaves the watched list', async () => {
  const a = await app();
  const prompt = a.ui.taskPrompt({ id: 3, projects: ['removed'], title: 'Fix the build',
    project_snapshots: [{ id: 'removed', name: 'Original project', path: 'C:/original' }] });
  assert.match(prompt, /Original project \(C:\/original\)/);
  assert.doesNotMatch(prompt, /neues Projekt/);
  assert.match(prompt, /watchtower tasks report 3/);
  a.state.projects = [];
  a.state.tasks = [{ id: 3, revision: 2, projects: ['removed'], status: 'review', title: 'Fix the build', result: 'Build fixed',
    project_snapshots: [{ id: 'removed', name: 'Original project', path: 'C:/original' }], created_at: '2026-10-07T12:00:00Z' }];
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /Build fixed/);
  assert.match(a.node('#view').innerHTML, /Ergebnis bestätigen/);
});

test('confirmation sends the displayed task revision and refreshes a changed report', async () => {
  const a = await app((command, args, state) => {
    if (command === 'set_task_status' && args.revision !== state.tasks[0].revision) throw { code: 'task_changed', message: 'Report changed' };
    return {};
  });
  a.state.tasks = [{ id: 4, revision: 2, projects: ['sample'], source_id: 's1', title: 'Original request', status: 'review',
    result: 'First result', created_at: '2026-10-07T12:00:00Z' }];
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /data-task-revision="2"/);
  a.state.tasks[0].revision = 3;
  a.state.tasks[0].result = 'Changed result';
  await a.ui.changeTaskStatus(4, 'done', 2);
  assert.equal(a.calls.find(c => c.command === 'set_task_status').args.revision, 2);
  assert.match(a.node('#view').innerHTML, /Changed result/);
  assert.match(a.node('#toast').innerHTML, /Prüfe zuerst das neue Ergebnis/);
});

test('routine Git stays neutral while acknowledging a collision reduces attention without hiding facts', async () => {
  const a = await app((command, args, state) => {
    if (command === 'set_warning_ack') {
      const warning = state.projects[0].warnings.find(w => w.key === args.key);
      warning.acknowledged = !!args.note; warning.note = args.note;
    }
    return {};
  });
  const p = a.state.projects[0];
  p.card.waiting_on_you = [];
  p.facts = { is_git: true, has_remote: false, dirty: { files: 2 }, commits: { weekly: Array(26).fill(0) }, remotes: [] };
  p.warnings = [{ key: 'no_remote', title: 'Kein Remote', acknowledged: false }];
  a.state.prefs.style = 'board';
  await a.ui.load(false);
  const before = a.ui.risk(p);
  assert.equal(before, 0);
  a.dispatch('input', { target: { value: 'Intentionally local', dataset: { warningNote: 'sample:no_remote' }, matches: () => false } });
  await a.ui.saveWarning('sample', 'no_remote');
  assert.equal(a.ui.risk(p), before);
  assert.match(a.node('#view').innerHTML, /bewusst ohne Remote/);
  assert.match(a.node('#view').innerHTML, /2 Dateien, \+0\/−0/);
  a.ui.openProject('sample');
  assert.match(a.node('#project-page').innerHTML, /Intentionally local/);
  await a.ui.saveWarning('sample', 'no_remote', true);
  assert.equal(a.ui.risk(p), before);
  p.facts.collisions = [{ a: { kind: 'main' }, b: { kind: 'worktree', name: 'feature' }, count: 1, files: ['src/lib.rs'] }];
  p.warnings.push({ key: 'worktrees', title: 'Weitere Arbeitslinien', acknowledged: false });
  await a.ui.load(false);
  assert.ok(a.ui.risk(p) > before);
  a.dispatch('input', { target: { value: 'Coordinated edits', dataset: { warningNote: 'sample:worktrees' }, matches: () => false } });
  await a.ui.saveWarning('sample', 'worktrees');
  assert.equal(a.ui.risk(p), before);
  assert.match(a.node('#project-page').innerHTML, /src\/lib\.rs/);
  assert.match(a.node('#project-page').innerHTML, /Coordinated edits/);
  p.warnings.push({ key: 'ci_red', title: 'CI ist fehlgeschlagen', can_acknowledge: false });
  await a.ui.load(false);
  assert.match(a.node('#project-page').innerHTML, /CI ist fehlgeschlagen/);
  assert.doesNotMatch(a.node('#project-page').innerHTML, /data-warning-open="ci_red"/);
});

test('read acknowledgement uses the displayed token and is never automatic', async () => {
  const a = await app();
  a.state.changes = { token: 'shown-snapshot', has_baseline: true, total: 1,
    entries: [{ project: 'sample', kind: 'decision_applied', title: 'Your answer is applied', next_step: 'Ship' }] };
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /Antwort übernommen/);
  assert.match(a.node('#view').innerHTML, /data-changes-read="shown-snapshot"/);
  assert.equal(a.calls.filter(c => c.command === 'mark_changes_read').length, 0);
  a.state.changes.token = 'newer-snapshot';
  await a.ui.markChangesRead('shown-snapshot');
  assert.equal(a.calls.find(c => c.command === 'mark_changes_read').args.token, 'shown-snapshot');
  a.state.changes = { token: 'empty', has_baseline: true, since: '2026-10-07T10:00:00Z', entries: [], total: 0 };
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /digest-empty faint.*07\.10\.2026/);
  assert.doesNotMatch(a.node('#view').innerHTML, /data-mod="changes"/);
});

test('report checks stay compact, concrete and distinct from Git freshness in every layout', async () => {
  const a = await app();
  a.state.projects[0].facts_collected_at = '2026-10-07T09:00:00Z';
  a.state.run_status = { state: 'complete', pending_owner_replies: 0,
    last_successful: { finished_at: '2026-10-07T07:30:00Z' },
    latest: { started_at: '2026-10-07T07:00:00Z', finished_at: '2026-10-07T07:30:00Z', counts: { total: 2, current: 2 },
      projects: [{ id: 'sample', status: 'current', initial_version: 4, card_version: 5 },
        { id: 'other', name: 'Other project', status: 'current', initial_version: 3, card_version: 3 }],
      portfolio: { status: 'current', initial_version: 8, version: 9 } } };
  for (const style of ['handover', 'board', 'ideas']) {
    a.state.prefs.style = style;
    await a.ui.load(false);
    assert.equal(a.node('#report-status-summary').textContent, 'Berichte geprüft');
    assert.equal(a.node('#report-status').hidden, false);
    assert.match(a.node('#fresh').textContent, /^Git-Fakten vor .* Min$/);
    assert.doesNotMatch(a.node('#view').innerHTML, /run-status|report-status|Agent-Lauf abgeschlossen/);
    const details = a.node('#report-status-body').innerHTML;
    assert.match(details, /Letzte vollständige Prüfung: 07\.10\.2026, \d{2}:\d{2} Uhr/);
    assert.match(details, /Berichte aktualisiert \(1\):.*Sample/);
    assert.match(details, /Unverändert übernommen \(1\):.*Other project/);
    assert.match(details, /Projektüberblick neu geschrieben\. Er bündelt Prioritäten, Verbindungen und Ideen/);
    assert.doesNotMatch(details, /Gesamtbrief|Karten im Lauf aktuell/);
  }
  a.node('#report-status').open = true;
  a.state.run_status.latest.portfolio.version = 8;
  await a.ui.load(false);
  assert.equal(a.node('#report-status').open, true);
  assert.match(a.node('#report-status-body').innerHTML, /Projektüberblick geprüft und unverändert übernommen/);
  a.dispatch('keydown', { target: { id: '', closest: () => null }, key: 'Escape', preventDefault() {} });
  assert.equal(a.node('#report-status').open, false);
  assert.equal(a.document.activeElement, a.node('#report-status-summary'));
  a.node('#report-status').open = true;
  a.dispatch('click', { target: { closest: () => null } });
  assert.equal(a.node('#report-status').open, false);
  assert.equal(a.calls.filter(c => c.command === 'set_pref').length, 0);
});

test('incomplete report checks and waiting replies stay visible with actionable details', async () => {
  const a = await app();
  a.state.run_status = { state: 'incomplete', pending_owner_replies: 2, repair_task: 'Continue this run',
    latest: { started_at: '2026-10-07T07:00:00Z', counts: { total: 2, current: 1 },
      projects: [{ id: 'sample', status: 'pending', reason: 'no_card' }], portfolio: { status: 'pending', reason: 'missing' } } };
  for (const style of ['handover', 'board', 'ideas']) {
    a.state.prefs.style = style;
    await a.ui.load(false);
    assert.equal(a.node('#report-status-summary').textContent, 'Prüfung unvollständig · 2 Antworten offen');
    const details = a.node('#report-status-body').innerHTML;
    assert.match(details, /2 Antworten warten/);
    assert.match(details, /data-copy="Continue this run"/);
    assert.match(details, /Einschätzung fehlt/);
    assert.match(details, /Projektüberblick fehlt/);
    assert.match(details, /Aktuelle Prüfung begonnen: 07\.10\.2026, \d{2}:\d{2} Uhr/);
    assert.doesNotMatch(details, /no_card|· missing/);
    assert.doesNotMatch(a.node('#view').innerHTML, /run-status|report-status/);
  }
  a.state.run_status.state = 'failed';
  a.state.run_status.latest.projects[0] = { id: 'sample', status: 'failed', error: 'path_missing' };
  await a.ui.load(false);
  assert.match(a.node('#report-status-summary').textContent, /Prüfung fehlgeschlagen/);
  assert.match(a.node('#report-status-body').innerHTML, /Projektordner fehlt/);
  a.state.run_status.state = 'complete';
  await a.ui.load(false);
  assert.match(a.node('#report-status-summary').textContent, /2 Antworten offen/);
  assert.equal(a.node('#report-status').attributes['data-state'], 'incomplete');
});

test('older check records do not pretend reports were rewritten or invent a completion time', async () => {
  const a = await app();
  a.state.run_status = { state: 'complete', latest: { started_at: '2026-10-07T07:00:00Z',
    projects: [{ id: 'sample', status: 'current' }], portfolio: { status: 'current' } } };
  await a.ui.load(false);
  let details = a.node('#report-status-body').innerHTML;
  assert.match(details, /Abschlusszeit nicht erfasst/);
  assert.match(details, /Berichte geprüft \(1\)/);
  assert.match(details, /Ob diese Berichte überarbeitet wurden, ist nicht erfasst/);
  assert.match(details, /Projektüberblick geprüft\. Ob er überarbeitet wurde, ist nicht erfasst/);
  assert.doesNotMatch(details, /Berichte aktualisiert|Unverändert übernommen|07\.10\.2026/);
  a.state.run_status = { state: 'unknown' };
  await a.ui.load(false);
  assert.equal(a.node('#report-status-summary').textContent, 'Berichte noch ungeprüft');
  details = a.node('#report-status-body').innerHTML;
  assert.match(details, /„Git-Fakten messen“ erneuert keine Agent-Berichte/);
});

test('recent activity puts newer same-day reports before the initial limit without promoting older events', async () => {
  const a = await app();
  const earlier = a.state.projects[0];
  earlier.card_updated_at = '2026-10-07T10:00:00Z';
  earlier.card.recent_changes = Array.from({ length: 12 }, (_, i) => ({ date: '2026-10-07', title: `Earlier report ${i}` }));
  const latest = structuredClone(earlier);
  latest.id = 'latest'; latest.name = 'Latest'; latest.card_updated_at = '2026-10-07T11:00:00Z';
  latest.card.recent_changes = [
    { date: '2026-10-06', title: 'Older event in the newest card' },
    { date: '2026-10-07', title: 'Latest report first' },
    { date: '2026-10-07', title: 'Latest report second' },
  ];
  a.state.projects.push(latest);
  await a.ui.load(false);
  const initial = a.ui.blockRecent();
  assert.match(initial, /Latest report first/);
  assert.match(initial, /Latest report second/);
  assert.ok(initial.indexOf('Latest report first') < initial.indexOf('Latest report second'));
  assert.ok(initial.indexOf('Latest report second') < initial.indexOf('Earlier report 0'));
  assert.match(initial, /Earlier report 7/);
  assert.doesNotMatch(initial, /Earlier report 8|Older event in the newest card/);

  const complete = a.ui.blockRecent(100);
  assert.ok(complete.indexOf('Earlier report 11') < complete.indexOf('Older event in the newest card'));
  assert.equal(latest.card.recent_changes[0].title, 'Older event in the newest card');
});

test('project recent activity is newest first and keeps the authored order within a date', async () => {
  const a = await app();
  a.state.projects[0].card.recent_changes = [
    { date: '2026-10-06', title: 'Earlier project event' },
    { date: '2026-10-07', title: 'Latest project event first' },
    { date: '2026-10-07', title: 'Latest project event second' },
  ];
  await a.ui.load(false);
  a.ui.openProject('sample');
  const html = a.node('#project-page').innerHTML;
  assert.ok(html.indexOf('Latest project event first') < html.indexOf('Latest project event second'));
  assert.ok(html.indexOf('Latest project event second') < html.indexOf('Earlier project event'));
  assert.equal(a.state.projects[0].card.recent_changes[0].title, 'Earlier project event');
});
