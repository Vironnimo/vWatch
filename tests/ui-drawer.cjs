const test = require('node:test');
const assert = require('node:assert/strict');
const { app } = require('./ui-helpers.cjs');

const expose = `Object.assign(ui, { drawerHtml, selectDrawerTab, openDrawerTarget, closeDrawer, searchEntries, pickResult, openSearch, renderSearchResults,
  search: query => { searchQ = query; return searchResults(); },
  ownerEditing: id => drawerEditors.has(id),
  allRatings: () => showRated,
});`;

function target(a, anchor) {
  const n = a.node(`#drawer-root [data-search-anchor="${anchor}"]`);
  n.classList = { add(value) { n.highlight = value; } };
  n.scrollIntoView = options => { n.scrolled = options; };
  return n;
}
const panel = (html, tab) => html.match(new RegExp(`<section id="drawer-panel-${tab}"[^>]*>`))[0];
const click = (a, selector, dataset = {}) => a.dispatch('click', { target: { closest: value => value === selector ? { dataset } : null } });

test('the project introduction preserves its voice while detail sections and owner fields start compact', async () => {
  const a = await app(undefined, expose), p = a.state.projects[0];
  p.card.one_liner = 'A useful project';
  p.card.personality = 'I have something to say';
  p.card.vision = { statement: 'Reach a useful release', progress_pct: 72, progress_note: 'Two milestones remain' };
  p.card.handbook = { run: [{ what: 'Start', how: 'cargo run' }] };
  p.facts = { is_git: true, commits: {}, remotes: [], weekly: [] };
  await a.ui.load(false);
  a.ui.openDrawer('sample');
  const html = a.node('#drawer-root').innerHTML;
  assert.match(html, /A useful project/);
  assert.match(html, /I have something to say/);
  assert.ok(html.indexOf('Aktueller Stand') < html.indexOf('drawer-tabs'));
  assert.ok(html.indexOf('Nächster Schritt') < html.indexOf('drawer-tabs'));
  assert.doesNotMatch(panel(html, 'work'), /hidden/);
  assert.match(panel(html, 'handbook'), / hidden/);
  assert.match(panel(html, 'history'), / hidden/);
  assert.match(html, /id="project-owner-fields" hidden/);
  assert.match(html, /Status und Ziel bearbeiten/);
  assert.ok(html.indexOf('Agent-Einschätzung: 72 %') > html.indexOf('id="drawer-panel-history"'));
  assert.match(html, /<details class="commit-history"[^>]*><summary>Commit-Aktivität ansehen/);
  assert.ok(html.indexOf('cargo run') > html.indexOf('id="drawer-panel-handbook"'));
});

test('project section and explicit editing remain open across refresh without losing drafts', async () => {
  const a = await app(undefined, expose);
  a.ui.openDrawer('sample');
  click(a, '[data-drawer-edit]');
  a.dispatch('input', { target: { id: 'goal-text', value: 'Ship a reviewed release', matches: () => false } });
  a.dispatch('input', { target: { id: 'status-note', value: 'Keep the local workflow', matches: () => false } });
  a.ui.selectDrawerTab('handbook');
  await a.ui.load(false);
  let html = a.node('#drawer-root').innerHTML;
  assert.doesNotMatch(panel(html, 'handbook'), /hidden/);
  assert.match(html, /value="Ship a reviewed release"/);
  assert.match(html, /value="Keep the local workflow"/);
  assert.equal(a.ui.ownerEditing('sample'), true);
  assert.doesNotMatch(html, /id="project-owner-fields" hidden/);
  a.ui.closeDrawer(); a.ui.openDrawer('sample');
  html = a.node('#drawer-root').innerHTML;
  assert.doesNotMatch(panel(html, 'handbook'), /hidden/);
  assert.equal(a.calls.filter(c => ['set_status', 'set_goal'].includes(c.command)).length, 0);
  a.dispatch('keydown', { key: 'ArrowRight', preventDefault() {}, target: { dataset: { drawerTab: 'handbook' } } });
  assert.doesNotMatch(panel(a.node('#drawer-root').innerHTML, 'history'), /hidden/);
});

test('search routes to the exact command, reply, task and owner idea instead of the top of a project', async () => {
  const a = await app(undefined, expose), p = a.state.projects[0];
  p.card.handbook = { run: [{ what: 'Open the app', how: 'cargo run' }, { what: 'Import', how: 'tool import --archive' }] };
  p.card.recent_changes = [{ date: '2026-10-01', title: 'Earlier archive work' }, { date: '2026-10-07', title: 'New release' }];
  a.state.feedback.q2 = { value: 'answered', note: 'Use the stable channel' };
  a.state.feedback.old_question = { value: 'answered', note: 'Keep archive compatibility', project_id: 'sample', at: '2026-10-01', item: { kind: 'question', title: 'Support old exports?' } };
  a.state.tasks = [{ id: 9, source_id: 's1', projects: ['sample'], title: 'Review exports', result: 'Roundtrip succeeded', status: 'review' }];
  a.state.ideas = [{ id: 4, text: 'My offline sketchbook', added_at: '2026-10-07' }];
  await a.ui.load(false);
  let results = a.ui.search('archive');
  const commandIndex = results.findIndex(r => r.where === 'Handbuch · Starten');
  assert.ok(commandIndex >= 0);
  const command = target(a, 'hb-run:1');
  a.ui.pickResult({ dataset: { sres: String(commandIndex) } });
  assert.equal(a.document.activeElement, command);
  assert.equal(command.highlight, 'search-hit');
  assert.equal(command.scrolled.block, 'center');
  assert.doesNotMatch(panel(a.node('#drawer-root').innerHTML, 'handbook'), /hidden/);
  assert.equal(a.ui.search('stable channel')[0].anchor, 'question:q2');
  assert.equal(a.ui.search('archive compatibility')[0].anchor, 'answer:old_question');
  assert.equal(a.ui.search('archive compatibility')[0].tab, 'history');
  assert.match(a.node('#drawer-root').innerHTML, /data-search-anchor="answer:old_question"/);
  assert.equal(a.ui.search('Roundtrip')[0].anchor, 'task:9');
  assert.equal(a.ui.search('offline sketchbook')[0].ideaId, 'idea:4');
  assert.equal(a.ui.search('Earlier archive')[0].anchor, 'recent:0');
  assert.match(a.node('#drawer-root').innerHTML, /data-search-anchor="recent:1"[^]*data-search-anchor="recent:0"/);
});

test('opening a hidden suggestion from search reveals only that destination and keeps rating filters intact', async () => {
  const a = await app(undefined, expose);
  a.state.feedback.s1 = { value: 'down' };
  await a.ui.load(false);
  const suggestion = target(a, 'suggestion:s1');
  const parentDetails = { tagName: 'DETAILS', open: false, parentElement: null };
  const evidence = { open: false };
  suggestion.parentElement = parentDetails;
  suggestion.querySelectorAll = () => [evidence];
  a.ui.openDrawerTarget('sample', 'work', 'suggestion:s1');
  assert.match(a.node('#drawer-root').innerHTML, /data-search-anchor="suggestion:s1"/);
  assert.equal(parentDetails.open, true);
  assert.equal(evidence.open, true);
  assert.equal(a.ui.allRatings(), false);
  await a.ui.load(false);
  assert.match(a.node('#drawer-root').innerHTML, /data-search-anchor="suggestion:s1"/);
  a.ui.closeDrawer(); a.ui.openDrawer('sample');
  assert.doesNotMatch(a.node('#drawer-root').innerHTML, /data-search-anchor="suggestion:s1"/);
});

test('projects without reports still expose an editable owner intent, folder and measured facts', async () => {
  const a = await app(undefined, expose);
  a.state.projects[0].card = null;
  await a.ui.load(false);
  a.ui.openDrawer('sample');
  const html = a.node('#drawer-root').innerHTML;
  assert.match(html, /Noch keine Einschätzung/);
  assert.match(html, /Status und Ziel bearbeiten/);
  assert.match(html, /data-search-anchor="path"/);
  assert.match(html, /data-search-anchor="facts"/);
  assert.equal(a.ui.search('C:/sample')[0].tab, 'handbook');
});

test('a visible search result keeps its identity when a background refresh inserts an earlier match', async () => {
  const a = await app(undefined, expose), alpha = a.state.projects[0];
  alpha.name = 'Alpha'; alpha.card.one_liner = 'Needle alpha';
  const beta = structuredClone(alpha); beta.id = 'beta'; beta.name = 'Beta'; beta.card.one_liner = 'Needle beta';
  a.state.projects.push(beta);
  await a.ui.load(false);
  a.node('#search-root .sresults [aria-selected="true"]').scrollIntoView = () => {};
  a.ui.search('Needle');
  a.ui.openSearch();
  const originalResults = a.node('#search-root .sresults').innerHTML;
  assert.match(originalResults, /data-sres="0"[^]*Alpha[^]*data-sres="1"[^]*Beta/);
  const earlier = structuredClone(alpha); earlier.id = 'earlier'; earlier.name = 'Earlier'; earlier.card.one_liner = 'Needle inserted';
  a.state.projects.unshift(earlier);
  await a.ui.load(false);
  assert.equal(a.node('#search-root .sresults').innerHTML, originalResults);
  // Keyboard selection rerenders the visible list but must not silently replace its entries.
  a.ui.renderSearchResults();
  assert.equal(a.node('#search-root .sresults').innerHTML, originalResults);
  const destination = target(a, 'purpose');
  a.ui.pickResult({ dataset: { sres: '1' } });
  assert.match(a.node('#drawer-root').innerHTML, /aria-label="Beta"/);
  assert.equal(a.document.activeElement, destination);
  a.ui.openSearch();
  assert.match(a.node('#search-root .sresults').innerHTML, /data-sres="0"[^]*Earlier/);
});

test('a visible handbook hit follows its content through insertions and reordering and refuses removed or ambiguous targets', async () => {
  const a = await app(undefined, expose), card = a.state.projects[0].card;
  const alpha = { what: 'Alpha', how: 'needle alpha', folder: '.' };
  const beta = { what: 'Beta', how: 'needle beta', folder: 'tools' };
  const inserted = { what: 'Inserted', how: 'needle inserted', folder: '.' };
  card.handbook = { run: [alpha, beta] };
  await a.ui.load(false);
  a.node('#search-root .sresults [aria-selected="true"]').scrollIntoView = () => {};
  a.ui.search('needle'); a.ui.openSearch();
  const originalResults = a.node('#search-root .sresults').innerHTML;
  card.handbook.run.unshift(inserted);
  await a.ui.load(false);
  assert.equal(a.node('#search-root .sresults').innerHTML, originalResults);
  const shiftedBeta = target(a, 'hb-run:2');
  a.ui.pickResult({ dataset: { sres: '1' } });
  assert.equal(a.document.activeElement, shiftedBeta);

  a.ui.closeDrawer(); a.ui.openSearch();
  card.handbook.run = [beta, inserted, alpha];
  await a.ui.load(false);
  const reorderedBeta = target(a, 'hb-run:0');
  a.ui.pickResult({ dataset: { sres: '2' } });
  assert.equal(a.document.activeElement, reorderedBeta);

  a.ui.closeDrawer(); a.ui.openSearch();
  card.handbook.run = [inserted, alpha];
  await a.ui.load(false);
  const visitsBefore = a.calls.filter(c => c.command === 'open_project').length;
  a.ui.pickResult({ dataset: { sres: '0' } });
  assert.equal(a.calls.filter(c => c.command === 'open_project').length, visitsBefore);
  assert.equal(a.node('#drawer-root').innerHTML, '');
  assert.match(a.node('#toast').innerHTML, /Treffer nicht mehr aktuell/);

  card.handbook.run = [beta];
  await a.ui.load(false); a.ui.openSearch();
  card.handbook.run.push(structuredClone(beta));
  await a.ui.load(false);
  a.ui.pickResult({ dataset: { sres: '0' } });
  assert.equal(a.calls.filter(c => c.command === 'open_project').length, visitsBefore);
  assert.match(a.node('#toast').innerHTML, /nicht mehr eindeutig/);
});
