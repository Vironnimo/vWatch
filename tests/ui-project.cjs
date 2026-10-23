const test = require('node:test');
const assert = require('node:assert/strict');
const { app } = require('./ui-helpers.cjs');

const expose = `Object.assign(ui, { projectPageHtml, openProjectTarget, searchEntries, pickResult, openSearch, renderSearchResults, blockSpace,
  search: query => { searchQ = query; return searchResults(); },
  ownerEditing: id => projectEditors.has(id),
  allRatings: () => showRated,
});`;

function target(a, anchor) {
  const n = a.node(`#project-page [data-search-anchor="${anchor}"]`);
  n.classList = { add(value) { n.highlight = value; } };
  n.scrollIntoView = options => { n.scrolled = options; };
  return n;
}
const page = a => a.node('#project-page').innerHTML;
// one part of the project page, from its heading to the next part
const group = (html, key) => html.slice(html.indexOf(`id="project-group-${key}"`)).split('<section id="project-group-')[0];
const click = (a, selector, dataset = {}) => a.dispatch('click', { target: { closest: value => value === selector ? { dataset } : null } });

test('incomplete and unverified line and space totals stay unknown while complete directory sizes remain usable', async () => {
  const a = await app(undefined, expose), p = a.state.projects[0];
  for (const verified of [false, undefined]) {
    p.facts = { is_git: true, dirty: { files: 3, lines_added: 0, lines_removed: 0, line_counts: { ok: verified } },
      space: { ok: verified, total_bytes: 0, dirs: [{ path: 'target', bytes: 0, ok: false }, { path: 'app/node_modules', bytes: 3e9, ok: true }] } };
    await a.ui.load(false);
    const html = a.ui.projectPageHtml(p), space = a.ui.blockSpace();
    assert.match(html, /3 Dateien, Zeilenzahl unbekannt/);
    assert.doesNotMatch(html, /\+0\/−0/);
    assert.match(html, /Größe unbekannt – nicht vollständig gemessen/);
    assert.match(space, /Gesamtgröße unbekannt/);
    assert.match(space, /app\/node_modules/);
    assert.match(space, /3(?:[.,]0)? GB/);
    assert.doesNotMatch(space, /0 MB/);
  }
  p.facts.dirty.line_counts.ok = true;
  p.facts.space = { ok: true, total_bytes: 0, dirs: [] };
  const html = a.ui.projectPageHtml(p);
  assert.match(html, /\+0\/−0/);
  assert.match(html, /0 MB neu erzeugbar/);
  p.facts.space = { ok: true, total_bytes: 3e9, dirs: [] };
  a.state.projects.push({ ...structuredClone(p), id: 'unmeasured', name: 'Not measured', facts: {} });
  await a.ui.load(false);
  assert.match(a.ui.blockSpace(), /Gesamtgröße unbekannt/);
  assert.match(a.ui.blockSpace(), /Not measured/);
});

test('a project opens as a page beside the project list, with work, handbook and history on one page', async () => {
  const a = await app(undefined, expose), p = a.state.projects[0];
  p.card.one_liner = 'A useful project';
  p.card.personality = 'I have something to say';
  p.card.vision = { statement: 'Reach a useful release', progress_pct: 72, progress_note: 'Two milestones remain' };
  p.card.handbook = { run: [{ what: 'Start', how: 'cargo run' }] };
  p.card.usage = { summary: 'Runs every morning from the scheduler', last_used: '2026-10-19', evidence: 'logs/run.log' };
  p.facts = { is_git: true, commits: {}, remotes: [], weekly: [] };
  await a.ui.load(false);
  a.ui.openProject('sample');
  assert.deepEqual(a.calls.filter(c => c.command === 'set_pref').map(c => [c.args.key, c.args.value]), [['style', 'board']]);
  assert.equal(a.calls.filter(c => c.command === 'open_project').length, 1);
  const view = a.node('#view').innerHTML, html = page(a);
  assert.match(view, /class="project-list"[^]*data-open="sample" aria-current="page"/);
  assert.match(view, /data-project-overview/);
  assert.doesNotMatch(view, /class="tile"/);
  assert.match(html, /A useful project/);
  assert.match(html, /I have something to say/);
  assert.ok(html.indexOf('Aktueller Stand') < html.indexOf('project-jump'));
  assert.ok(html.indexOf('Nächster Schritt') < html.indexOf('project-jump'));
  assert.ok(html.indexOf('Im Alltag') < html.indexOf('project-jump'));
  assert.match(html, /Runs every morning from the scheduler[^]*zuletzt genutzt 19\.10\.2026/);
  for (const key of ['work', 'handbook', 'history']) assert.match(html, new RegExp(`<section id="project-group-${key}" class="project-group"`));
  assert.doesNotMatch(html, /<section id="project-group-[a-z]+"[^>]*hidden/);
  assert.match(html, /id="project-owner-fields" hidden/);
  assert.match(html, /Status und Ziel bearbeiten/);
  assert.match(group(html, 'history'), /Agent-Einschätzung: 72 %/);
  assert.match(group(html, 'history'), /<details class="commit-history"[^>]*><summary>Commit-Aktivität ansehen/);
  assert.match(group(html, 'handbook'), /cargo run/);
  assert.doesNotMatch(group(html, 'work'), /cargo run/);
});

test('owner fields stay open across refresh without losing drafts, and the overview returns explicitly', async () => {
  const a = await app(undefined, expose);
  a.ui.openProject('sample');
  click(a, '[data-project-intent-edit]');
  a.dispatch('input', { target: { id: 'goal-text', value: 'Ship a reviewed release', matches: () => false } });
  a.dispatch('input', { target: { id: 'status-note', value: 'Keep the local workflow', matches: () => false } });
  await a.ui.load(false);
  let html = page(a);
  assert.match(html, /value="Ship a reviewed release"/);
  assert.match(html, /value="Keep the local workflow"/);
  assert.equal(a.ui.ownerEditing('sample'), true);
  assert.doesNotMatch(html, /id="project-owner-fields" hidden/);
  assert.equal(a.calls.filter(c => ['set_status', 'set_goal'].includes(c.command)).length, 0);
  const visits = a.calls.filter(c => c.command === 'open_project').length;
  await a.ui.load(false);
  assert.equal(a.calls.filter(c => c.command === 'open_project').length, visits, 'a refresh is not a visit');
  click(a, '[data-project-overview]');
  assert.match(a.node('#view').innerHTML, /class="tile"/);
  assert.doesNotMatch(a.node('#view').innerHTML, /id="project-page"/);
  a.ui.openProject('sample');
  a.dispatch('keydown', { key: 'Escape', preventDefault() {}, target: { id: '', dataset: {}, closest: () => null } });
  assert.doesNotMatch(a.node('#view').innerHTML, /id="project-page"/);
});

test('status buttons change the project intent, while task cards with a status do not', async () => {
  const a = await app(command => command === 'set_status' ? { undo_token: 'opaque verdict receipt / v3' } : {}, expose + 'Object.assign(ui, { adoptVerdict });');
  a.state.tasks = [{ id: 3, source_id: 's1', projects: ['sample'], title: 'Review exports', status: 'review' }];
  await a.ui.load(false);
  a.ui.openProject('sample');
  a.dispatch('click', { target: { closest: selector => selector === '[data-status]' ? { dataset: { status: 'review' } } : null } });
  assert.equal(a.calls.filter(c => c.command === 'set_status').length, 0);
  a.dispatch('click', { target: { closest: selector => selector === '.seg [data-status]' ? { dataset: { status: 'parked' } } : null } });
  await new Promise(resolve => setImmediate(resolve));
  assert.deepEqual(a.calls.filter(c => c.command === 'set_status').map(c => c.args.status), ['parked']);
  await a.ui.adoptVerdict('sample', 'active');
  a.node('#undo').onclick();
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(a.calls.find(c => c.command === 'undo').args.token, 'opaque verdict receipt / v3');
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
  assert.match(group(page(a), 'handbook'), /data-search-anchor="hb-run:1"/);
  assert.equal(a.ui.search('stable channel')[0].anchor, 'question:q2');
  assert.equal(a.ui.search('archive compatibility')[0].anchor, 'answer:old_question');
  assert.equal(a.ui.search('archive compatibility')[0].tab, 'history');
  assert.match(page(a), /data-search-anchor="answer:old_question"/);
  assert.equal(a.ui.search('Roundtrip')[0].anchor, 'task:9');
  assert.equal(a.ui.search('offline sketchbook')[0].ideaId, 'idea:4');
  assert.equal(a.ui.search('Earlier archive')[0].anchor, 'recent:0');
  assert.match(page(a), /data-search-anchor="recent:1"[^]*data-search-anchor="recent:0"/);
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
  a.ui.openProjectTarget('sample', 'work', 'suggestion:s1');
  assert.match(page(a), /data-search-anchor="suggestion:s1"/);
  assert.equal(parentDetails.open, true);
  assert.equal(evidence.open, true);
  assert.equal(a.ui.allRatings(), false);
  await a.ui.load(false);
  assert.match(page(a), /data-search-anchor="suggestion:s1"/);
  a.ui.closeProject(); a.ui.openProject('sample');
  assert.doesNotMatch(page(a), /data-search-anchor="suggestion:s1"/);
});

test('projects without reports still expose an editable owner intent, folder and measured facts', async () => {
  const a = await app(undefined, expose);
  a.state.projects[0].card = null;
  await a.ui.load(false);
  a.ui.openProject('sample');
  const html = page(a);
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
  assert.match(page(a), /aria-label="Beta"/);
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

  a.ui.closeProject(); a.ui.openSearch();
  card.handbook.run = [beta, inserted, alpha];
  await a.ui.load(false);
  const reorderedBeta = target(a, 'hb-run:0');
  a.ui.pickResult({ dataset: { sres: '2' } });
  assert.equal(a.document.activeElement, reorderedBeta);

  a.ui.closeProject(); a.ui.openSearch();
  card.handbook.run = [inserted, alpha];
  await a.ui.load(false);
  const visitsBefore = a.calls.filter(c => c.command === 'open_project').length;
  a.ui.pickResult({ dataset: { sres: '0' } });
  assert.equal(a.calls.filter(c => c.command === 'open_project').length, visitsBefore);
  assert.doesNotMatch(a.node('#view').innerHTML, /id="project-page"/);
  assert.match(a.node('#toast').innerHTML, /Treffer nicht mehr aktuell/);

  card.handbook.run = [beta];
  await a.ui.load(false); a.ui.openSearch();
  card.handbook.run.push(structuredClone(beta));
  await a.ui.load(false);
  a.ui.pickResult({ dataset: { sres: '0' } });
  assert.equal(a.calls.filter(c => c.command === 'open_project').length, visitsBefore);
  assert.match(a.node('#toast').innerHTML, /nicht mehr eindeutig/);
});

test('an older stored Handbuch preference opens Projects without rewriting the preference', async () => {
  const a = await app(undefined, expose);
  a.state.prefs.style = 'handbook';
  await a.ui.load(false);
  assert.match(a.node('#view').innerHTML, /class="tile"/);
  assert.equal(a.calls.filter(c => c.command === 'set_pref').length, 0);
});
