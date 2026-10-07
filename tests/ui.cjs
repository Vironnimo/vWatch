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
  render, blockIdeas, openDrawer, taskPrompt
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
  const state = fixture(), nodes = new Map(), events = new Map(), calls = [];
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
    window: {
      addEventListener() {}, scrollTo() {},
      __TAURI__: { core: { invoke: async (command, args) => {
        calls.push({ command, args });
        return command === 'state' ? structuredClone(state) : invoke(command, args);
      } } },
    },
  });
  vm.runInContext(source.replace('load(true).then(refreshFacts);', expose), context);
  await context.ui.load(false);
  return { ui: context.ui, state, node, calls, dispatch: (type, event) => {
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
