import { S, call, errText, inputFocus, load, render, restoreInputFocus, toast } from './app.js';
import { rated, showRated } from './feedback.js';
import { $, MOD_TITLES, STATUS, ago, byId, collapsedMods, dot, esc, mod, plink } from './presentation.js';

let panelOpen = false;  // settings panel
let settings = null;    // autostart, agent access, paths
const projectEdits = {}; // unsaved names and folder paths in settings
let disc = null;        // project search: { target, result (from discover), checked:Set, busy }
let confirmRemove = null;

// ---------- page ----------
function renderFoot() {
  const available = new Set(S.prefs.style === 'board' ? ['projects', 'tasks', 'recent', 'week', 'across', 'safety', 'night', 'space'] : S.prefs.style === 'handover' ? ['focus', 'changes'] : []);
  const collapsed = [...collapsedMods()].filter(id => available.has(id));
  const ratedCount = Object.keys(S.feedback).filter(id => rated(id) === 'down' || rated(id) === 'later').length;
  const archived = S.projects.filter(p => p.status === 'archived');
  const parts = [];
  if (collapsed.length) parts.push(`Eingeklappt: ${collapsed.map(id => `${esc(MOD_TITLES[id] || id)} <button type="button" data-mod-toggle="${esc(id)}">ausklappen</button>`).join(' · ')}`);
  if (ratedCount && S.prefs.style === 'board') parts.push(`Bewertete Einschätzungen in „Verlauf und Einordnung“ <button type="button" data-toggle-rated>${showRated ? 'wieder ausblenden' : 'einblenden'}</button>`);
  if (archived.length) parts.push(`Archiviert: ${archived.map(p => plink(p.id)).join(', ')}`);
  parts.push(`Karten schreiben Agents mit <code class="mono">vwatch</code> · Fakten misst die App selbst`);
  $('#foot').innerHTML = parts.map(x => `<span>${x}</span>`).join('');
}

// ---------- adding projects ----------
// while a search runs or its checklist is open, the list replaces the add button
const discBusyOrListing = target => disc && disc.target === target && (disc.busy || disc.result.candidates.length);
function discHtml(target) {
  if (!disc || disc.target !== target) return '';
  if (disc.busy) return `<p class="muted disc-msg">${esc(disc.busy)}</p>`;
  const r = disc.result, folder = `<span class="mono">${esc(r.folder)}</span>`;
  if (!r.candidates.length) {
    const msg = r.already_watched
      ? (r.folder_is_repo ? `${folder} ist schon im Blick.` : `Alle Git-Repos in ${folder} sind schon im Blick.`)
      : `In ${folder} habe ich keine Git-Repos gefunden. Wähle den Ordner, in dem deine Projekte liegen, oder direkt ein Projekt.`;
    return `<p class="muted disc-msg">${msg}</p>`;
  }
  const n = disc.checked.size, all = n === r.candidates.length;
  const head = r.folder_is_repo
    ? (r.candidates.length > 1 ? `<b>${esc(r.candidates[0].name)}</b> ist ausgewählt. Daneben liegen noch diese Repos, falls du sie auch willst:` : `<b>${esc(r.candidates[0].name)}</b> ist ausgewählt.`)
    : `${r.candidates.length} Git-Repos gefunden. Angehakt sind die, an denen zuletzt gearbeitet wurde.`;
  const rows = r.candidates.map(c => `<label class="disc-row"><input type="checkbox" data-disc="${esc(c.path)}" ${disc.checked.has(c.path) ? 'checked' : ''}><span class="t">${esc(c.name)}</span><span class="faint">${c.last_commit ? `letzter Commit ${ago(c.last_commit)}` : 'noch kein Commit'}</span></label>`).join('');
  return `<div class="disc"><div class="disc-head"><span>${head}</span>${r.candidates.length > 1 ? `<button class="linkish" type="button" data-disc-all>${all ? 'keins' : 'alle'} auswählen</button>` : ''}</div>
    <div class="disc-list">${rows}</div>
    <div class="actions"><button class="btn primary" type="button" data-disc-add ${n ? '' : 'disabled'}>${n === 1 ? '1 Projekt' : `${n} Projekte`} hinzufügen</button><button class="btn" type="button" data-disc-cancel>Abbrechen</button></div></div>`;
}

function renderEmpty() {
  return `<div class="empty"><div class="kicker">Willkommen</div><h2>Welche Projekte soll vWatch im Blick behalten?</h2>
    <p class="muted">Wähle den Ordner, in dem deine Projekte liegen, oder direkt ein einzelnes Projekt. vWatch zeigt dir die gefundenen Git-Repos zum Abhaken und misst sofort, wie es um sie steht. Die Einschätzungen schreibt danach ein Agent; wie du den einrichtest, steht in den Einstellungen.</p>
    ${discBusyOrListing('main') ? '' : '<div class="actions"><button class="btn primary" type="button" data-pick data-target="main">Ordner wählen …</button></div>'}
    ${discHtml('main')}</div>`;
}

// ---------- settings panel ----------
function toggle(key, on, label, sub) {
  return `<div class="setting"><div><div class="t">${esc(label)}</div>${sub ? `<div class="d">${esc(sub)}</div>` : ''}</div><button class="toggle" type="button" role="switch" aria-checked="${on}" data-setting="${key}" aria-label="${esc(label)}"><span></span></button></div>`;
}
const copyLine = (text, label) => `<div class="copyline"><code class="mono">${esc(text)}</code><button class="btn" type="button" data-copy="${esc(text)}" aria-label="${esc(label)} kopieren">Kopieren</button></div>`;

function projectEditor(p) {
  const edit = projectEdits[p.id];
  if (!edit) return '';
  const disabled = edit.busy ? ' disabled' : '';
  return `<div class="project-editor" aria-busy="${!!edit.busy}">
    <label for="project-name-${esc(p.id)}">Name in vWatch</label>
    <input type="text" id="project-name-${esc(p.id)}" data-project-edit-field="name" data-project="${esc(p.id)}" value="${esc(edit.name)}"${disabled}>
    <label for="project-path-${esc(p.id)}">Projektordner</label>
    <div class="inrow"><input type="text" class="mono" id="project-path-${esc(p.id)}" data-project-edit-field="path" data-project="${esc(p.id)}" value="${esc(edit.path)}"${disabled}><button class="btn" type="button" data-project-folder="${esc(p.id)}"${disabled}>Ordner wählen …</button></div>
    <p class="d">Karten, Antworten und Verlauf bleiben erhalten. Wenn du den Ordner bereits verschoben oder umbenannt hast, wähle seinen neuen Ort. vWatch verschiebt keine Dateien.</p>
    ${edit.error ? `<p class="project-edit-error bad-t" role="alert">${esc(edit.error)}</p>` : ''}
    <div class="actions"><button class="btn primary" type="button" data-project-save="${esc(p.id)}"${disabled}>${edit.busy === 'save' ? 'Speichert …' : 'Speichern'}</button><button class="btn" type="button" data-project-cancel="${esc(p.id)}"${disabled}>Abbrechen</button></div>
  </div>`;
}

function panelHtml() {
  const st = settings || {};
  const projects = S.projects.map(p => `<div class="prow"><div class="pmain"><div class="pname-s">${dot(p.id)}<b>${esc(p.name)}</b>${p.status !== 'active' ? `<span class="tag">${esc(STATUS[p.status])}</span>` : ''}</div><div class="d mono">${esc(p.path)}</div></div>
    ${projectEdits[p.id] ? '' : confirmRemove === p.id
      ? `<div class="confirm"><span>Nicht mehr beobachten? Karten, Antworten und Verlauf bleiben gespeichert.</span><button class="btn danger" type="button" data-remove="${esc(p.id)}">Entfernen</button><button class="btn" type="button" data-remove-cancel>Behalten</button></div>`
      : `<div class="project-actions"><button class="btn" type="button" data-project-edit="${esc(p.id)}">Bearbeiten</button><button class="linkish" type="button" data-remove-ask="${esc(p.id)}">entfernen</button></div>`}${projectEditor(p)}</div>`).join('');
  return `<div class="scrim" data-close-panel></div><aside class="drawer panel" role="dialog" aria-modal="true" aria-label="Einstellungen">
    <button class="close" data-close-panel type="button">Schließen</button>
    <h2 class="pname">Einstellungen</h2>
    <section class="mod"><div class="mod-head"><div><h2>Projekte</h2><div class="sub">${S.projects.length ? `${S.projects.length} im Blick. Status und Notiz setzt du auf der Seite eines Projekts.` : 'Noch keine.'}</div></div></div>
      <div class="plist">${projects}</div>
      ${discBusyOrListing('panel') ? '' : '<div class="actions"><button class="btn" type="button" data-pick data-target="panel">Projekte hinzufügen …</button></div>'}
      ${discHtml('panel')}</section>
    <section class="mod"><div class="mod-head"><div><h2>Start</h2></div></div>
      ${toggle('autostart', !!st.autostart, 'Beim Anmelden öffnen', 'vWatch geht auf, sobald du dich bei Windows anmeldest.')}</section>
    <section class="mod"><div class="mod-head"><div><h2>Agents</h2><div class="sub">Agents lesen deine Repos und schreiben die Einschätzungen mit dem Befehl „vwatch“.</div></div></div>
      ${toggle('agent_access', !!st.agent_access, 'Für Agents aufrufbar', 'Trägt den vWatch-Ordner in deinen PATH ein. Programme, die schon laufen (etwa vBot), sehen das erst nach einem Neustart.')}
      <div class="setting-block"><div class="t">Skill für Agents</div><div class="d">Erklärt einem Agent den täglichen Lauf. vWatch hält ihn bei jedem Start aktuell.</div>${copyLine(st.skill_dir || '', 'Pfad')}</div>
      <div class="setting-block"><div class="t">In vBot installieren</div><div class="d">Gib den Befehl dem Agent, der vWatch betreuen soll. Er installiert den Skill nur für sich. Danach kannst du ihm den täglichen Lauf zuweisen.</div>${copyLine(st.vbot_install || '', 'Befehl')}</div></section>
    <section class="mod"><div class="mod-head"><div><h2>Über</h2></div></div>
      <dl class="kv"><dt>Version</dt><dd class="mono">${esc(st.version || '')}</dd><dt>Programm</dt><dd class="mono">${esc(st.exe || '')}</dd><dt>Daten</dt><dd class="mono">${esc(st.data_dir || '')}</dd></dl></section>
  </aside>`;
}
function renderPanel(keepScroll) {
  const root = $('#panel-root');
  if (!panelOpen) { root.innerHTML = ''; return; }
  const prev = root.querySelector('.drawer');
  const top = keepScroll && prev ? prev.scrollTop : 0;
  const focused = inputFocus();
  root.innerHTML = panelHtml();
  root.querySelector('.drawer').scrollTop = top;
  restoreInputFocus(focused);
}
async function openPanel() {
  panelOpen = true;
  renderPanel(false);
  try { settings = await call('settings'); renderPanel(true); } catch (e) { toast(errText(e)); }
}
function closePanel() {
  panelOpen = false;
  confirmRemove = null;
  if (disc && disc.target === 'panel') disc = null;
  renderPanel(false);
}


function rerenderDisc(target = disc && disc.target) { if (target === 'panel') renderPanel(true); else render(); }
async function pick(target) {
  let path;
  try { path = await call('pick_folder'); } catch (e) { toast(errText(e)); return; }
  if (!path) return;
  disc = { target, busy: 'Suche Git-Repos …' };
  rerenderDisc();
  try {
    const result = await call('discover', { root: path });
    disc = { target, result, checked: new Set(result.candidates.filter(c => c.recommended).map(c => c.path)), busy: null };
  } catch (e) { disc = null; toast(errText(e)); }
  rerenderDisc();
}
async function addPaths(paths, target) {
  disc = { ...(disc || { target, checked: new Set() }), busy: paths.length === 1 ? 'Füge hinzu und messe …' : `Füge ${paths.length} Projekte hinzu und messe …` };
  rerenderDisc();
  try {
    const res = await call('add_projects', { paths });
    disc = null;
    await load(false);
    if (res.failed.length) toast(res.failed.map(f => f.error.message).join(' '));
    else toast(res.added.length === 1 ? `${res.added[0].name} ist jetzt im Blick.` : `${res.added.length} Projekte sind jetzt im Blick.`);
  } catch (e) { disc.busy = null; rerenderDisc(); toast(errText(e)); }
}
async function setSetting(key, on) {
  try {
    settings = await call(key === 'autostart' ? 'set_autostart' : 'set_agent_access', { on });
    renderPanel(true);
  } catch (e) { toast('Nicht geändert: ' + errText(e)); }
}
function editProject(id) {
  const p = byId(id);
  if (!p) return;
  projectEdits[id] ||= { name: p.name, path: p.path, originalName: p.name, originalPath: p.path, busy: null, error: null };
  confirmRemove = null;
  renderPanel(true);
  $(`#project-name-${CSS.escape(id)}`).focus();
}
function cancelProjectEdit(id) {
  if (!projectEdits[id] || projectEdits[id].busy) return;
  delete projectEdits[id];
  renderPanel(true);
}
async function pickProjectFolder(id) {
  const edit = projectEdits[id];
  if (!edit || edit.busy) return;
  edit.busy = 'pick'; edit.error = null;
  renderPanel(true);
  try {
    const path = await call('pick_folder');
    if (path) edit.path = path;
  } catch (e) { edit.error = `Ordner konnte nicht gewählt werden: ${errText(e)}`; }
  finally { edit.busy = null; renderPanel(true); }
}
async function saveProjectEdit(id) {
  const edit = projectEdits[id];
  if (!edit || edit.busy) return;
  const name = edit.name.trim(), path = edit.path.trim();
  if (!name || !path) {
    edit.error = !name ? 'Gib einen Namen ein.' : 'Gib den vorhandenen Projektordner an.';
    renderPanel(true); return;
  }
  const args = { id };
  if (name !== edit.originalName) args.name = name;
  if (path !== edit.originalPath) args.path = path;
  if (!args.name && !args.path) { cancelProjectEdit(id); return; }
  edit.busy = 'save'; edit.error = null;
  renderPanel(true);
  try {
    const result = await call('update_project', args);
    delete projectEdits[id];
    await load(false);
    toast(result.facts_refresh && result.facts_refresh.ok === false
      ? 'Projekt geändert. Fakten konnten noch nicht neu gemessen werden. Versuche es mit „Git-Fakten messen“ erneut.'
      : 'Projekt geändert. Karten und Verlauf bleiben erhalten.');
  } catch (e) {
    edit.busy = null;
    edit.error = `Nicht gespeichert: ${errText(e)}`;
    renderPanel(true);
  }
}
async function removeProject(id) {
  const p = byId(id);
  try {
    await call('remove_project', { id });
    confirmRemove = null;
    await load(false);
    toast(`${p ? p.name : id} ist nicht mehr im Blick. Karten und Verlauf bleiben gespeichert.`, async () => {
      try {
        await call('restore_project', { id });
        await load(false);
        toast(`${p ? p.name : id} ist wieder im Blick.`);
      } catch (e) { toast(`Nicht wiederhergestellt: ${errText(e)}`); }
    });
  } catch (e) { toast(errText(e)); }
}


function handleSettingsClick(e, t) {
  if (t.closest('[data-open-settings]')) { openPanel(); return true; }
  if (t.closest('[data-close-panel]')) { closePanel(); return true; }
  const pk = t.closest('[data-pick]');
  if (pk) { pick(pk.dataset.target); return true; }
  const pe = t.closest('[data-project-edit]');
  if (pe) { editProject(pe.dataset.projectEdit); return true; }
  const pf = t.closest('[data-project-folder]');
  if (pf) { pickProjectFolder(pf.dataset.projectFolder); return true; }
  const ps = t.closest('[data-project-save]');
  if (ps) { saveProjectEdit(ps.dataset.projectSave); return true; }
  const pc = t.closest('[data-project-cancel]');
  if (pc) { cancelProjectEdit(pc.dataset.projectCancel); return true; }
  if (t.closest('[data-disc-all]')) { const all = disc.result.candidates; disc.checked = disc.checked.size === all.length ? new Set() : new Set(all.map(c => c.path)); rerenderDisc(); return true; }
  if (t.closest('[data-disc-add]')) { addPaths([...disc.checked], disc.target); return true; }
  if (t.closest('[data-disc-cancel]')) { const target = disc.target; disc = null; rerenderDisc(target); return true; }
  const setting = t.closest('[data-setting]');
  if (setting) { setSetting(setting.dataset.setting, setting.getAttribute('aria-checked') !== 'true'); return true; }
  const ask = t.closest('[data-remove-ask]');
  if (ask) { confirmRemove = ask.dataset.removeAsk; renderPanel(true); return true; }
  if (t.closest('[data-remove-cancel]')) { confirmRemove = null; renderPanel(true); return true; }
  const rm = t.closest('[data-remove]');
  if (rm) { removeProject(rm.dataset.remove); return true; }
  return false;
}

function handleSettingsChange(e) {
  if (e.target.matches('[data-disc]')) {
    const path = e.target.dataset.disc;
    if (e.target.checked) disc.checked.add(path); else disc.checked.delete(path);
    rerenderDisc();
    return true;
  }
  return false;
}

function handleSettingsInput(e) {
  const editField = e.target.dataset && e.target.dataset.projectEditField;
  const edit = editField && projectEdits[e.target.dataset.project];
  if (edit && !edit.busy && (editField === 'name' || editField === 'path')) edit[editField] = e.target.value;
  return false;
}

function handleSettingsKey(e) {
  if (e.target.dataset && e.target.dataset.projectEditField && (e.key === 'Enter' || e.key === 'Escape')) {
    e.preventDefault();
    if (e.key === 'Enter') saveProjectEdit(e.target.dataset.project);
    else cancelProjectEdit(e.target.dataset.project);
    return true;
  }
  return false;
}

export { closePanel, handleSettingsChange, handleSettingsClick, handleSettingsInput, handleSettingsKey, panelOpen, renderEmpty, renderFoot, renderPanel };
