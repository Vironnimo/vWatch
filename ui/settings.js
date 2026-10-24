import { tr } from './i18n.js';
import { S, call, errText, inputFocus, load, render, restoreInputFocus, setPref, toast } from './app.js';
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
  if (collapsed.length) parts.push(tr("Collapsed: {0}", collapsed.map(id => `${esc(MOD_TITLES[id] || id)} <button type="button" data-mod-toggle="${esc(id)}">${tr("expand")}</button>`).join(' · ')));
  if (ratedCount && S.prefs.style === 'board') parts.push(`${tr("Rated assessments in “History and context”")} <button type="button" data-toggle-rated>${showRated ? tr("hide again") : tr("show")}</button>`);
  if (archived.length) parts.push(tr("Archived: {0}", archived.map(p => plink(p.id)).join(', ')));
  parts.push(`${tr("Agents write Cards using")} <code class="mono">vwatch</code> ${tr("· the app measures facts itself")}`);
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
      ? (r.folder_is_repo ? tr("{0} is already being watched.", folder) : tr("All Git repositories in {0} are already being watched.", folder))
      : tr("No Git repositories found in {0}. Choose your Projects folder or a Project directly.", folder);
    return `<p class="muted disc-msg">${msg}</p>`;
  }
  const n = disc.checked.size, all = n === r.candidates.length;
  const head = r.folder_is_repo
    ? (r.candidates.length > 1 ? `<b>${esc(r.candidates[0].name)}</b> ${tr("is selected. Nearby repositories, if you want them too:")}` : `<b>${esc(r.candidates[0].name)}</b> ${tr("is selected.")}`)
    : tr("Found {0} Git repositories. Recently active ones are checked.", r.candidates.length);
  const rows = r.candidates.map(c => `<label class="disc-row"><input type="checkbox" data-disc="${esc(c.path)}" ${disc.checked.has(c.path) ? 'checked' : ''}><span class="t">${esc(c.name)}</span><span class="faint">${c.last_commit ? tr("last commit {0}", ago(c.last_commit)) : tr("no commit yet")}</span></label>`).join('');
  return `<div class="disc"><div class="disc-head"><span>${head}</span>${r.candidates.length > 1 ? `<button class="linkish" type="button" data-disc-all>${tr("Select {0}", all ? tr("none") : tr("all"))}</button>` : ''}</div>
    <div class="disc-list">${rows}</div>
    <div class="actions"><button class="btn primary" type="button" data-disc-add ${n ? '' : 'disabled'}>${tr("Add {0}", n === 1 ? tr("1 Project") : tr("{0} Projects", n))}</button><button class="btn" type="button" data-disc-cancel>${tr("Cancel")}</button></div></div>`;
}

function renderEmpty() {
  return `<div class="empty"><div class="kicker">${tr("Welcome")}</div><h2>${tr("Which Projects should vWatch watch?")}</h2>
    <p class="muted">${tr("Choose your Projects folder or a single Project. vWatch lists the Git repositories for you to select and immediately measures their state. An Agent writes the assessments afterwards; find setup instructions in Settings.")}</p>
    ${discBusyOrListing('main') ? '' : `<div class="actions"><button class="btn primary" type="button" data-pick data-target="main">${tr("Choose folder …")}</button></div>`}
    ${discHtml('main')}</div>`;
}

// ---------- settings panel ----------
function toggle(key, on, label, sub) {
  return `<div class="setting"><div><div class="t">${esc(label)}</div>${sub ? `<div class="d">${esc(sub)}</div>` : ''}</div><button class="toggle" type="button" role="switch" aria-checked="${on}" data-setting="${key}" aria-label="${esc(label)}"><span></span></button></div>`;
}
const copyLine = (text, label) => `<div class="copyline"><code class="mono">${esc(text)}</code><button class="btn" type="button" data-copy="${esc(text)}" aria-label="${tr("Copy {0}", esc(label))}">${tr("Copy")}</button></div>`;

function projectEditor(p) {
  const edit = projectEdits[p.id];
  if (!edit) return '';
  const disabled = edit.busy ? ' disabled' : '';
  return `<div class="project-editor" aria-busy="${!!edit.busy}">
    <label for="project-name-${esc(p.id)}">${tr("Name in vWatch")}</label>
    <input type="text" id="project-name-${esc(p.id)}" data-project-edit-field="name" data-project="${esc(p.id)}" value="${esc(edit.name)}"${disabled}>
    <label for="project-path-${esc(p.id)}">${tr("Project folder")}</label>
    <div class="inrow"><input type="text" class="mono" id="project-path-${esc(p.id)}" data-project-edit-field="path" data-project="${esc(p.id)}" value="${esc(edit.path)}"${disabled}><button class="btn" type="button" data-project-folder="${esc(p.id)}"${disabled}>${tr("Choose folder …")}</button></div>
    <p class="d">${tr("Cards, answers and history are preserved. If you already moved or renamed the folder, choose its new location. vWatch does not move files.")}</p>
    ${edit.error ? `<p class="project-edit-error bad-t" role="alert">${esc(edit.error)}</p>` : ''}
    <div class="actions"><button class="btn primary" type="button" data-project-save="${esc(p.id)}"${disabled}>${edit.busy === 'save' ? tr("Saving …\u0004project") : tr("Save\u0004button")}</button><button class="btn" type="button" data-project-cancel="${esc(p.id)}"${disabled}>${tr("Cancel")}</button></div>
  </div>`;
}

function panelHtml() {
  const st = settings || {};
  const projects = S.projects.map(p => `<div class="prow"><div class="pmain"><div class="pname-s">${dot(p.id)}<b>${esc(p.name)}</b>${p.status !== 'active' ? `<span class="tag">${esc(STATUS[p.status])}</span>` : ''}</div><div class="d mono">${esc(p.path)}</div></div>
    ${projectEdits[p.id] ? '' : confirmRemove === p.id
      ? `<div class="confirm"><span>${tr("Stop watching? Cards, answers and history stay saved.")}</span><button class="btn danger" type="button" data-remove="${esc(p.id)}">${tr("Remove")}</button><button class="btn" type="button" data-remove-cancel>${tr("Keep")}</button></div>`
      : `<div class="project-actions"><button class="btn" type="button" data-project-edit="${esc(p.id)}">${tr("Edit")}</button><button class="linkish" type="button" data-remove-ask="${esc(p.id)}">${tr("remove")}</button></div>`}${projectEditor(p)}</div>`).join('');
  return `<div class="scrim" data-close-panel></div><aside class="drawer panel" role="dialog" aria-modal="true" aria-label="${tr("Settings")}">
    <button class="close" data-close-panel type="button">${tr("Close")}</button>
    <h2 class="pname">${tr("Settings")}</h2>
    <section class="mod"><div class="mod-head"><h2>${tr('Language')}</h2></div>
      <div class="setting"><label class="t" for="language">${tr('Window and new reports')}</label>
      <select id="language" data-language><option value="en"${S.prefs.language !== 'de' ? ' selected' : ''}>English</option><option value="de"${S.prefs.language === 'de' ? ' selected' : ''}>Deutsch</option></select></div>
      <p class="d">${tr('Saved reports keep their language. New or revised reports use your selection.')}</p></section>
    <section class="mod"><div class="mod-head"><div><h2>${tr("Projects")}</h2><div class="sub">${S.projects.length ? tr("Watching {0}. Set status and notes on a Project's page.", S.projects.length) : tr("None yet.")}</div></div></div>
      <div class="plist">${projects}</div>
      ${discBusyOrListing('panel') ? '' : `<div class="actions"><button class="btn" type="button" data-pick data-target="panel">${tr("Add Projects …")}</button></div>`}
      ${discHtml('panel')}</section>
    <section class="mod"><div class="mod-head"><div><h2>Start</h2></div></div>
      ${toggle('autostart', !!st.autostart, tr("Open at sign-in"), tr("vWatch opens when you sign in to Windows."))}</section>
    <section class="mod"><div class="mod-head"><div><h2>Agents</h2><div class="sub">${tr("Agents read your repositories and write assessments using the “vwatch” command.")}</div></div></div>
      ${toggle('agent_access', !!st.agent_access, tr("Available to Agents"), tr("Adds the vWatch folder to your PATH. Already running applications (such as vBot) see this after restarting."))}
      <div class="setting-block"><div class="t">${tr("Skill for Agents")}</div><div class="d">${tr("Explains the daily Run to an Agent. vWatch keeps it current at every startup.")}</div>${copyLine(st.skill_dir || '', tr("Path"))}</div>
      <div class="setting-block"><div class="t">${tr("Install in vBot")}</div><div class="d">${tr("Give this command to the Agent that will manage vWatch. It installs the Skill for itself. You can then assign it the daily Run.")}</div>${copyLine(st.vbot_install || '', tr("Command"))}</div></section>
    <section class="mod"><div class="mod-head"><div><h2>${tr("About")}</h2></div></div>
      <dl class="kv"><dt>Version</dt><dd class="mono">${esc(st.version || '')}</dd><dt>${tr("Application")}</dt><dd class="mono">${esc(st.exe || '')}</dd><dt>${tr("Data")}</dt><dd class="mono">${esc(st.data_dir || '')}</dd></dl></section>
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
  disc = { target, busy: tr("Searching Git repositories …") };
  rerenderDisc();
  try {
    const result = await call('discover', { root: path });
    disc = { target, result, checked: new Set(result.candidates.filter(c => c.recommended).map(c => c.path)), busy: null };
  } catch (e) { disc = null; toast(errText(e)); }
  rerenderDisc();
}
async function addPaths(paths, target) {
  disc = { ...(disc || { target, checked: new Set() }), busy: paths.length === 1 ? tr("Adding and measuring …") : tr("Adding and measuring {0} Projects …", paths.length) };
  rerenderDisc();
  try {
    const res = await call('add_projects', { paths });
    disc = null;
    await load(false);
    if (res.failed.length) toast(res.failed.map(f => f.error.message).join(' '));
    else toast(res.added.length === 1 ? tr("{0} is now being watched.", res.added[0].name) : tr("{0} Projects are now being watched.", res.added.length));
  } catch (e) { disc.busy = null; rerenderDisc(); toast(errText(e)); }
}
async function setSetting(key, on) {
  try {
    settings = await call(key === 'autostart' ? 'set_autostart' : 'set_agent_access', { on });
    renderPanel(true);
  } catch (e) { toast(`${tr("Not changed:")} ` + errText(e)); }
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
  } catch (e) { edit.error = tr("Folder could not be selected: {0}", errText(e)); }
  finally { edit.busy = null; renderPanel(true); }
}
async function saveProjectEdit(id) {
  const edit = projectEdits[id];
  if (!edit || edit.busy) return;
  const name = edit.name.trim(), path = edit.path.trim();
  if (!name || !path) {
    edit.error = !name ? tr("Enter a name.") : tr("Enter the existing Project folder.");
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
      ? tr("Project changed. Facts could not be measured yet. Try “Measure Git facts” again.")
      : tr("Project changed. Cards and history are preserved."));
  } catch (e) {
    edit.busy = null;
    edit.error = tr("Not saved: {0}", errText(e));
    renderPanel(true);
  }
}
async function removeProject(id) {
  const p = byId(id);
  try {
    await call('remove_project', { id });
    confirmRemove = null;
    await load(false);
    toast(tr("{0} is no longer being watched. Cards and history remain saved.", p ? p.name : id), async () => {
      try {
        await call('restore_project', { id });
        await load(false);
        toast(tr("{0} is being watched again.", p ? p.name : id));
      } catch (e) { toast(tr("Not restored: {0}", errText(e))); }
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
  if (e.target.matches('[data-language]')) { setPref('language', e.target.value); return true; }
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
