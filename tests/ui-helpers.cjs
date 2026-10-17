// Run with `node --test tests/ui.cjs`; no browser packages are required.
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
  render, blockIdeas, blockRecent, openProject, closeProject, startIdeaRound, ideaRoundStep, taskPrompt, assignTask, copyTask, changeTaskStatus, toggleModule,
  openSnooze, saveSnooze, rated, saveWarning, risk, markChangesRead,
  openPanel, closePanel, editProject, cancelProjectEdit, pickProjectFolder, saveProjectEdit,
  removeProject, removeIdea
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
        reentry: { next_step: 'Continue' },
        suggestions: [{ id: 's1', title: 'Suggestion', why: 'Useful', type: 'feature' }],
        waiting_on_you: [1, 2, 3].map(n => ({
          id: `q${n}`, title: `Question ${n}`, options: [{ answer: `Option ${n}` }],
        })),
      },
    }],
  };
}

async function app(invoke = () => ({}), extraExpose = '') {
  const state = fixture(), nodes = new Map(), events = new Map(), calls = [], copied = [];
  let document;
  const node = selector => {
    if (!nodes.has(selector)) nodes.set(selector, {
      innerHTML: '', value: '', textContent: '', hidden: false, scrollTop: 0,
      focus() { document.activeElement = this; }, select() {},
      setSelectionRange(start, end) { this.selectionStart = start; this.selectionEnd = end; },
      attributes: {}, addEventListener() {}, setAttribute(key, value) { this.attributes[key] = value; },
      querySelector: child => node(`${selector} ${child}`), querySelectorAll: () => [],
    });
    return nodes.get(selector);
  };
  document = {
    querySelector: node, querySelectorAll: selector => nodes.has(selector) ? [nodes.get(selector)] : [], activeElement: null,
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
        if (command === 'set_pref') state.prefs[args.key] = args.value;
        return command === 'state' ? structuredClone(state) : invoke(command, args, state);
      } } },
    },
  });
  vm.runInContext(source.replace('load(true).then(refreshWhenDue);', expose + extraExpose), context);
  await context.ui.load(false);
  return { ui: context.ui, state, node, calls, copied, document, dispatch: (type, event) => {
    for (const handler of events.get(type) || []) handler(event);
  } };
}

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

module.exports = { app, fixture, deferred };
