// Run with `node --test tests/ui.cjs`; no browser packages are required.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
const expose = `globalThis.ui = {
  load, startRound, closeRound, roundStep, setFeedback, saveNote, saveAnswer,
  highlight: (text, query) => { searchQ = query; return highlight(text); },
  draft: (id, text) => { drafts[id] = text; },
  note: id => { noteFor = id; },
  answer: id => { answering = id; },
  render, blockIdeas, openDrawer, taskPrompt, assignTask, copyTask, changeTaskStatus,
  openSnooze, saveSnooze, rated, saveWarning, risk, markChangesRead
};`;

function fixture() {
  return {
    today: '2026-10-07', review_week: '2026-W40', prefs: { style: 'handover' },
    feedback: {}, ideas: [], snooze_days: 7,
    projects: [{
      id: 'sample', name: 'Sample', path: 'C:/sample', status: 'active',
      card: {
        headline: 'Ready', phase: 'active', features: [], open_ends: [],
        recent_changes: [], milestones: [], health_notes: [], cross_project_hints: [],
        reentry: { next_step: 'Continue', effort: 'S' },
        suggestions: [{ id: 's1', title: 'Suggestion', why: 'Useful', type: 'feature' }],
        waiting_on_you: [1, 2, 3].map(n => ({
          id: `q${n}`, title: `Question ${n}`, options: [{ answer: `Option ${n}` }],
        })),
      },
    }],
  };
}

async function app(invoke = () => ({})) {
  const state = fixture(), nodes = new Map(), events = new Map(), calls = [], copied = [];
  let document;
  const node = selector => {
    if (!nodes.has(selector)) nodes.set(selector, {
      innerHTML: '', value: '', textContent: '', hidden: false, scrollTop: 0,
      focus() { document.activeElement = this; }, select() {}, setSelectionRange() {},
      addEventListener() {}, setAttribute() {},
      querySelector: child => node(`${selector} ${child}`), querySelectorAll: () => [],
    });
    return nodes.get(selector);
  };
  document = {
    querySelector: node, querySelectorAll: () => [], activeElement: null,
    addEventListener(type, handler) {
      const list = events.get(type) || [];
      list.push(handler); events.set(type, list);
    },
  };
  const context = vm.createContext({
    document, CSS: { escape: x => x }, setInterval() {}, setTimeout() {}, clearTimeout() {},
    navigator: { clipboard: { writeText: async text => { copied.push(text); } } },
    window: {
      addEventListener() {}, scrollTo() {},
      __TAURI__: { core: { invoke: async (command, args) => {
        calls.push({ command, args });
        return command === 'state' ? structuredClone(state) : invoke(command, args, state);
      } } },
    },
  });
  vm.runInContext(source.replace('load(true).then(refreshFacts);', expose), context);
  await context.ui.load(false);
  return { ui: context.ui, state, node, calls, copied, dispatch: (type, event) => {
    for (const handler of events.get(type) || []) handler(event);
  } };
}

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

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

test('later and down ratings leave the optional explanation visible on the overview', async () => {
  for (const value of ['later', 'down']) {
    const a = await app();
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
  const input = (id, value) => a.dispatch('input', {
    target: { id, value, matches: () => false },
  });
  input('idea-text', 'A useful new app');
  a.ui.openDrawer('sample');
  input('goal-text', 'Ship next month');
  input('status-note', 'Only fix bugs');
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /value="A useful new app"/);
  assert.match(a.node('#drawer-root').innerHTML, /value="Ship next month"/);
  assert.match(a.node('#drawer-root').innerHTML, /value="Only fix bugs"/);
});

test('a copied task labels today as export date instead of evidence date', async () => {
  const a = await app();
  const task = a.ui.taskPrompt({ projects: ['sample'], title: 'Old finding' });
  assert.match(task, /kopiert am 07\.10\.2026/);
  assert.doesNotMatch(task, /Stand 07\.10\.2026/);
});

test('later waits for an explicit future calendar date and preserves its note', async () => {
  const a = await app();
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

test('snoozes expire on the stored local date and undo restores the old deadline', async () => {
  const a = await app();
  const old = { value: 'later', note: 'Next week', at: '2026-10-01T23:00:00Z', snoozed_until: '2026-10-07' };
  a.state.feedback.s1 = old;
  await a.ui.load(false);
  assert.equal(a.ui.rated('s1'), null);
  a.ui.openSnooze('s1');
  await a.ui.saveSnooze('2026-10-14');
  a.node('#undo').onclick();
  await new Promise(setImmediate);
  const undo = a.calls.find(c => c.command === 'restore_feedback');
  assert.deepEqual(JSON.parse(JSON.stringify(undo.args.snapshot)), old);
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
  await a.ui.copyTask('s1');
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
  a.ui.openDrawer('sample');
  assert.match(a.node('#drawer-root').innerHTML, /abgeschlossene Aufträge/);
  assert.match(a.node('#drawer-root').innerHTML, /Original request/);
  assert.match(a.node('#drawer-root').innerHTML, /Wieder öffnen/);
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

test('acknowledging a warning reduces attention without hiding measured facts', async () => {
  const a = await app((command, args, state) => {
    if (command === 'set_warning_ack') {
      const warning = state.projects[0].warnings[0];
      warning.acknowledged = !!args.note; warning.note = args.note;
    }
    return {};
  });
  const p = a.state.projects[0];
  p.facts = { is_git: true, has_remote: false, dirty: { files: 2 }, commits: { weekly: Array(26).fill(0) }, remotes: [] };
  p.warnings = [{ key: 'no_remote', title: 'Kein Remote', acknowledged: false }];
  await a.ui.load(false);
  const before = a.ui.risk(p);
  a.dispatch('input', { target: { value: 'Intentionally local', dataset: { warningNote: 'sample:no_remote' }, matches: () => false } });
  await a.ui.saveWarning('sample', 'no_remote');
  assert.equal(a.ui.risk(p), before - 2);
  assert.match(a.node('#view').innerHTML, /bewusst ohne Remote/);
  assert.match(a.node('#view').innerHTML, /2 Dateien offen/);
  a.ui.openDrawer('sample');
  assert.match(a.node('#drawer-root').innerHTML, /Intentionally local/);
  await a.ui.saveWarning('sample', 'no_remote', true);
  assert.equal(a.ui.risk(p), before);
  p.warnings.push({ key: 'ci_red', title: 'CI ist fehlgeschlagen', can_acknowledge: false });
  await a.ui.load(false);
  assert.match(a.node('#drawer-root').innerHTML, /CI ist fehlgeschlagen/);
  assert.doesNotMatch(a.node('#drawer-root').innerHTML, /data-warning-open="ci_red"/);
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

test('agent status remains distinct from fact freshness in every layout', async () => {
  const a = await app();
  a.state.run_status = { state: 'incomplete', pending_owner_replies: 2, repair_task: 'Continue this run',
    latest: { started_at: '2026-10-07T07:00:00Z', counts: { total: 2, current: 1 },
      projects: [{ id: 'sample', status: 'pending', reason: 'no_card' }], portfolio: { status: 'pending', reason: 'missing' } } };
  for (const style of ['handover', 'board', 'handbook']) {
    a.state.prefs.style = style;
    await a.ui.load(false);
    assert.match(a.node('#view').innerHTML, /Agent-Lauf unvollständig/);
    assert.match(a.node('#view').innerHTML, /2 Antworten warten/);
    assert.match(a.node('#view').innerHTML, /data-copy="Continue this run"/);
    assert.match(a.node('#view').innerHTML, /Einschätzung fehlt/);
    assert.match(a.node('#view').innerHTML, /Gesamtbrief fehlt/);
    assert.doesNotMatch(a.node('#view').innerHTML, /no_card|· missing/);
  }
});
