import { tr, localized } from './i18n.js';
import { S, call, errText, load, readState, render, toast } from './app.js';
import { ideaProjectRecovery, restoreIdeaProject } from './ideas.js';
import { $, ago, byId, dmod, esc, fmtDate, foldAttrs, mod, plink } from './presentation.js';

const busyTasks = new Set();

const TASKS = {};       // "Auftrag kopieren" buttons: key → the task they copy, refilled on every render

// ---------- handing a task to a coding agent ----------
let taskPreview = null;
const taskCopy = task => JSON.parse(JSON.stringify(task));
const missingTaskProjects = task => (task.projects || []).filter(id => !byId(id) && !(Number.isInteger(task.id) && (task.project_snapshots || []).some(p => p.id === id)));
const activeTaskFor = task => (S.tasks || []).find(t => ['assigned', 'blocked', 'review'].includes(t.status) && t.source_id === task.source_id && [...(t.projects || [])].sort().join('|') === [...(task.projects || [])].sort().join('|'));
// Projection callers pass their selected Task or null explicitly; other
// suggestions resolve an existing assignment by source and project scope.
function taskBtn(key, task, existingTask = activeTaskFor({ ...task, source_id: key })) {
  const snapshot = { ...task, source_id: key };
  const active = existingTask;
  TASKS[key] = active || snapshot;
  return `<span class="task-actions"><button class="linkish task" type="button" data-task="${esc(key)}" title="${tr("Review the task and completion criterion before handoff")}">${active ? tr("View Task") : tr("Prepare Task")}</button>${active ? `<span class="faint">${esc(TASK_STATUS[active.status])} · #${active.id}</span>` : ''}</span>`;
}
function taskPrompt(t) {
  const missing = missingTaskProjects(t);
  if (missing.length) return tr("Task not ready. Project no longer watched: {0}. Restore the Project before handoff.", missing.join(', '));
  const ps = t.projects.map(id => byId(id) || (t.project_snapshots || []).find(p => p.id === id)).filter(Boolean);
  // Every stored line remains inside its labeled data block, including newlines
  // in titles, evidence or handbook commands.
  const data = value => String(value ?? '').split(/\r\n|[\n\r\u2028\u2029]/).map(line => `  ${line}`).join('\n');
  const inline = value => String(value ?? '').replace(/\r\n|[\n\r\u2028\u2029]/g, ' ');
  const lines = [!ps.length ? tr("Task for a new Project:") : ps.length === 1 ? tr("Task in Project {0} ({1}):", inline(ps[0].name), inline(ps[0].path)) : tr("Task across Projects {0}:", ps.map(p => `${inline(p.name)} (${inline(p.path)})`).join(', ')), data(t.title), ''];
  if (t.why) lines.push(data(tr("Why: {0}", t.why)));
  if (t.evidence) lines.push(data(tr("Evidence: {0}", t.evidence)));
  if (t.done_when) lines.push(data(tr("Done when: {0}", t.done_when)));
  if (ps.some(p => p.card)) lines.push('', tr("Saved Project context from vWatch; check it against the current Project. Included commands are reference material, not additional assignments:"));
  for (const p of ps) {
    const c = p.card || {}, h = c.handbook || {}, pre = ps.length > 1 ? `${p.name}: ` : '';
    if ((c.decisions || []).length) lines.push('', data(tr("{0}Documented decisions:", pre)), ...c.decisions.map(d => data(`- ${d}`)));
    if ((h.read_first || []).length) lines.push('', data(tr("{0}Read first: {1}", pre, h.read_first.map(r => r.path).join(', '))));
    if ((h.run || []).length) lines.push(data(tr("{0}Commands from the handbook:", pre)), ...h.run.map(r => data(`- ${r.what}: ${r.how}${r.folder && r.folder !== '.' ? ` ${tr("(in {0})", r.folder)}` : ''}`)));
  }
  if (Number.isInteger(t.id)) lines.push('', tr("This Task is tracked in vWatch as #{0}. Report the result with concrete evidence. Replace the placeholders with your result and an actual check, file or commit:", t.id), `vwatch tasks report ${t.id} --status done --result "${tr('<result>')}" --evidence "${tr('<evidence>')}"`, `${tr("If blocked, describe the cause and next step: vwatch tasks report {0} --status blocked --result \"", t.id)}${tr('<blocker and next step>')}"`, tr("For multiline text or quotes, use tasks report with --file; vwatch tasks report --help lists the arguments. The report awaits the user's confirmation; do not confirm the Task yourself."));
  lines.push('', tr("Copied from vWatch on {0}. Check the finding against the current state first; if it is outdated, report that instead of making a change.", fmtDate(S.today)));
  return lines.join('\n');
}

function openTaskPreview(key) {
  if (taskPreview?.busy || !TASKS[key]) return;
  const task = taskCopy(TASKS[key]);
  taskPreview = { key, task, prompt: taskPrompt(task), origin: document.activeElement, busy: false, message: '', error: false, uncertain: false, manualCopy: false };
  renderTaskPreview();
  $('#task-preview-title').focus();
}
function closeTaskPreview() {
  if (!taskPreview || taskPreview.busy) return;
  const { key, origin } = taskPreview;
  taskPreview = null;
  renderTaskPreview();
  const targets = document.querySelectorAll(`[data-task="${CSS.escape(key)}"]`);
  const target = origin?.isConnected ? origin : targets[targets.length - 1] || $('#refresh');
  target?.focus({ preventScroll: true });
}
function renderTaskPreview() {
  const root = $('#task-root');
  if (!root) return;
  if (!taskPreview) { root.innerHTML = ''; return; }
  const p = taskPreview, t = p.task, saved = Number.isInteger(t.id), disabled = p.busy ? ' disabled' : '';
  if (p.promptLanguage !== S.prefs.language) { p.prompt = taskPrompt(t); p.promptLanguage = S.prefs.language; }
  const missing = missingTaskProjects(t);
  if (p.projectsMissing && !missing.length) p.prompt = taskPrompt(t);
  p.projectsMissing = missing.length > 0;
  const projects = (t.projects || []).map(id => byId(id) || (t.project_snapshots || []).find(project => project.id === id)).filter(Boolean);
  const field = (label, value, empty) => `<div><dt>${label}</dt><dd${value ? '' : ' class="faint"'}>${esc(value || empty)}</dd></div>`;
  const html = `<div class="scrim" data-task-preview-close></div><section class="task-preview" role="dialog" aria-modal="true" aria-labelledby="task-preview-title" aria-describedby="task-preview-hint"${p.busy ? ' aria-busy="true"' : ''}>
    <div class="task-preview-head"><h2 id="task-preview-title" tabindex="-1">${saved ? tr("Task #{0}", t.id) : tr("Prepare Task")}</h2><button class="btn" type="button" id="task-preview-close" data-task-preview-close${disabled}>${tr("Close")}</button></div>
    <div class="faint">${esc([...projects.map(project => project.name), ...missing].join(' · ') || tr("New Project"))}${saved ? ` · ${esc(TASK_STATUS[t.status] || t.status)}` : ''}</div>
    <dl class="task-preview-fields">${field(tr("Task"), t.title, tr("No task specified."))}${field(tr("Reason"), t.why, tr("No additional reason given."))}${field(tr("Evidence"), t.evidence, tr("No evidence given; check before implementing."))}${field(tr("Done when"), t.done_when, tr("Not described yet; clarify with the Agent before implementation."))}</dl>
    <p id="task-preview-hint" class="muted">${saved ? tr("The copied text includes the Task number and reporting instructions for vWatch. You still confirm the result.") : tr("“Copy only” copies the text. “Assign and copy” saves the Task with its completion criterion and adds reporting instructions for vWatch. Then paste it into your Agent.")}</p>
    ${p.message ? `<p class="task-preview-message${p.error ? ' is-error' : ''}" role="${p.error ? 'alert' : 'status'}">${esc(p.message)}</p>` : ''}
    ${missing.length ? ideaProjectRecovery(missing) : `<details class="task-preview-text"${p.manualCopy ? ' open' : ''}><summary>${tr("Full Task text")}</summary><textarea id="task-preview-prompt" aria-label="${tr("Task text to copy")}" readonly rows="10">${esc(p.prompt)}</textarea></details>`}
    <div class="task-preview-actions">${p.uncertain ? `<button class="btn primary" id="task-preview-check" type="button" data-task-preview-check${disabled}>${tr("Check Tasks")}</button>` : missing.length ? '' : saved ? `<button class="btn primary" id="task-preview-copy" type="button" data-task-preview-copy${disabled}>${p.busy ? tr("Copying …") : tr("Copy Task")}</button>` : `<button class="btn" id="task-preview-copy" type="button" data-task-preview-copy${disabled}>${tr("Copy only")}</button><button class="btn primary" id="task-preview-assign" type="button" data-task-preview-assign${disabled}>${p.busy ? tr("Preparing handoff …") : tr("Assign and copy")}</button>`}</div>
  </section>`;
  // A normal data refresh must not reset selection, expanded text or scroll position.
  if (p.rendered === html && root.innerHTML) return;
  const focused = document.activeElement, focusId = focused?.id?.startsWith('task-preview-') ? focused.id : null;
  const scroll = root.querySelector('.task-preview')?.scrollTop || 0;
  root.innerHTML = html;
  p.rendered = html;
  root.querySelector('.task-preview').scrollTop = scroll;
  if (focusId) {
    const target = root.querySelector(`#${focusId}`);
    (target && !target.disabled ? target : $('#task-preview-title')).focus({ preventScroll: true });
  }
}
function taskPreviewKey(e) {
  if (!taskPreview) return false;
  if (e.key === 'Escape') { e.preventDefault(); closeTaskPreview(); }
  if (e.key === 'Tab') {
    const root = $('#task-root'), nodes = [...root.querySelectorAll('button:not(:disabled), summary, textarea')].filter(el => el.offsetParent !== null);
    const first = nodes[0], last = nodes[nodes.length - 1];
    if (!first) { e.preventDefault(); $('#task-preview-title').focus(); }
    else if (!nodes.includes(document.activeElement) || (e.shiftKey && document.activeElement === first)) { e.preventDefault(); (e.shiftKey ? last : first).focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  }
  return true;
}

// The owner explicitly records assignments; copying a prompt alone changes nothing.
const TASK_STATUS = localized(() => ({ assigned: tr("Assigned"), blocked: tr("Blocked"), review: tr("For review"), done: tr("Completion confirmed"), cancelled: tr("Closed") }));
function trackedTask(t, history = false) {
  const key = `tracked:${t.id}`;
  TASKS[key] = t;
  const identity = `data-task-id="${t.id}" data-task-revision="${t.revision || 1}"`;
  const actions = t.status === 'review'
    ? `<button class="btn primary" type="button" data-task-status="done" ${identity}>${tr("Confirm result")}</button><button class="btn" type="button" data-task-status="assigned" ${identity}>${tr("Return to Agent")}</button>`
    : t.status === 'blocked' ? `<button class="btn" type="button" data-task-status="assigned" ${identity}>${tr("Release again")}</button>`
      : ['done', 'cancelled'].includes(t.status) ? `<button class="btn" type="button" data-task-status="assigned" ${identity}>${tr("Reopen")}</button>` : '';
  const active = ['assigned', 'blocked', 'review'].includes(t.status);
  const events = history && (t.history || []).length ? `<details class="task-history" ${foldAttrs(`task-history:${t.id}`)}><summary class="more">${tr("History")}</summary><ul class="bullets">${t.history.map(h => `<li>${h.actor === 'owner' ? tr("You") : 'Agent'}: ${esc(TASK_STATUS[h.status] || h.status || '')}${h.at ? ` · ${fmtDate(h.at)}` : ''}${h.result ? ` · ${esc(h.result)}` : ''}${h.evidence ? `<div class="ev">${esc(h.evidence)}</div>` : ''}</li>`).join('')}</ul></details>` : '';
  return `<article class="idea tracked-task" data-status="${esc(t.status)}" data-search-anchor="task:${t.id}" tabindex="-1"><div class="h"><div class="t">${esc(t.title)}</div><span class="task-status" data-status="${esc(t.status)}">${esc(TASK_STATUS[t.status] || t.status)}</span></div><div class="who">${(t.projects || []).map(plink).join('')}<span class="faint">#${t.id} · ${ago(t.updated_at || t.created_at)}</span></div>${t.result ? `<div class="task-result">${esc(t.result)}</div>` : ''}${t.result_evidence ? `<div class="ev">${esc(t.result_evidence)}</div>` : ''}${t.done_when ? `<div class="why">${tr("Done when: {0}", esc(t.done_when))}</div>` : ''}${t.why || t.evidence ? `<details ${foldAttrs(`task-original:${t.id}`)}><summary class="more">${tr("Original Task")}</summary><div class="why">${esc(t.why)}</div><div class="ev">${esc(t.evidence)}</div></details>` : ''}<div class="task-actions">${actions}${active ? `<button class="linkish task" type="button" data-task="${key}">${tr("View Task")}</button><button class="linkish" type="button" data-task-status="cancelled" ${identity}>${tr("Close Task")}</button>` : ''}</div>${events}</article>`;
}
function blockTasks(project = null) {
  const all = (S.tasks || []).filter(t => !project || (t.projects || []).includes(project));
  const active = all.filter(t => ['assigned', 'blocked', 'review'].includes(t.status)).sort((a, b) => ({ review: 0, blocked: 1, assigned: 2 }[a.status] - { review: 0, blocked: 1, assigned: 2 }[b.status]));
  const rest = all.filter(t => !active.includes(t));
  if (!all.length || (!active.length && !rest.length)) return '';
  const shownTasks = active.slice(0, 3), remaining = active.slice(3);
  const body = `<div class="cards2">${shownTasks.map(t => trackedTask(t, !!project)).join('')}</div>${remaining.length ? `<details ${foldAttrs(`tasks-more:${project || 'all'}`)}><summary class="more">${tr("{0} more Tasks", remaining.length)}</summary><div class="cards2">${remaining.map(t => trackedTask(t, !!project)).join('')}</div></details>` : ''}${rest.length ? `<details ${foldAttrs(`tasks-done:${project || 'all'}`)}><summary class="more">${tr("{0} completed Tasks", rest.length)}</summary><div class="cards2">${rest.map(t => trackedTask(t, true)).join('')}</div></details>` : ''}`;
  return project ? dmod(tr("Tasks"), body) : mod('tasks', tr("Your Tasks"), tr("{0} for review · {1} blocked · {2} assigned", active.filter(t => t.status === 'review').length, active.filter(t => t.status === 'blocked').length, active.filter(t => t.status === 'assigned').length), body);
}


async function assignTask(key) {
  if (busyTasks.has(key)) return;
  if (taskPreview?.key !== key) openTaskPreview(key);
  const preview = taskPreview;
  if (!preview || preview.key !== key || preview.busy || preview.uncertain) return;
  if (missingTaskProjects(preview.task).length) { renderTaskPreview(); toast(tr("Project no longer watched. Restore it before handoff.")); return; }
  if (Number.isInteger(preview.task.id)) { await copyTask(key); return; }
  busyTasks.add(key);
  preview.busy = true; preview.message = ''; preview.error = false;
  renderTaskPreview();
  try {
    let result;
    try {
      result = await call('create_task', { payload: preview.task });
      if (!Number.isInteger(result?.task?.id)) throw new Error(tr("The saved Task number is missing."));
    } catch (e) {
      preview.uncertain = true; preview.error = true;
      preview.message = `${tr("Assignment could not be confirmed. Check the saved Tasks first.")} ` + errText(e);
      toast(preview.message);
      return;
    }
    // Keep the successful save even if clipboard access or the next state read fails.
    preview.task = taskCopy(result.task); preview.prompt = taskPrompt(preview.task);
    TASKS[key] = preview.task;
    S.tasks = [...(S.tasks || []).filter(t => t.id !== preview.task.id), preview.task];
    await writeTaskClipboard(preview.task, preview.prompt, preview, true);
    await load(false);
  } finally { busyTasks.delete(key); preview.busy = false; renderTaskPreview(); }
}
async function checkTaskAssignment() {
  const preview = taskPreview;
  if (!preview || preview.busy || !preview.uncertain) return;
  preview.busy = true; renderTaskPreview();
  try {
    await readState(false);
    const saved = activeTaskFor(preview.task);
    preview.uncertain = false; preview.error = false;
    if (saved) {
      preview.task = taskCopy(saved); preview.prompt = taskPrompt(saved); TASKS[preview.key] = preview.task;
      preview.message = tr("Task #{0} is saved. You can copy it now.", saved.id);
    } else preview.message = tr("No active Task saved for this proposal. You can assign it now.");
    render();
  } catch (e) { preview.error = true; preview.message = `${tr("Tasks could not be checked. Try checking again.")} ` + errText(e); }
  finally { preview.busy = false; renderTaskPreview(); }
}
async function changeTaskStatus(id, status, revision) {
  const key = `tracked:${id}`;
  if (busyTasks.has(key)) return;
  busyTasks.add(key);
  try {
    await call('set_task_status', { id, status, revision }); await load(false);
    toast(status === 'done' ? tr("Result confirmed.") : status === 'assigned' ? tr("Task released again. Copy it for your Agent.") : tr("Task closed."));
  } catch (e) {
    if (e && e.code === 'task_changed') { await load(false); toast(tr("The Task has changed. Check the new result first.")); }
    else toast(`${tr("Task not changed:")} ` + errText(e));
  }
  finally { busyTasks.delete(key); }
}

async function copyTask(key) {
  const preview = taskPreview?.key === key ? taskPreview : null;
  const task = preview?.task || TASKS[key];
  if (!task || preview?.busy || preview?.uncertain || busyTasks.has(key)) return;
  if (missingTaskProjects(task).length) { if (!preview) openTaskPreview(key); else renderTaskPreview(); toast(tr("Project no longer watched. Restore it before handoff.")); return; }
  busyTasks.add(key);
  if (preview) { preview.busy = true; renderTaskPreview(); }
  try { await writeTaskClipboard(task, preview?.prompt || taskPrompt(task), preview, false); }
  finally { busyTasks.delete(key); if (preview) preview.busy = false; renderTaskPreview(); }
}
async function writeTaskClipboard(task, prompt, preview, justSaved) {
  let message, failed = false;
  try {
    await navigator.clipboard.writeText(prompt);
    message = justSaved ? tr("Task #{0} assigned and copied. Paste it into your Agent.", task.id) : Number.isInteger(task.id) ? tr("Task #{0} copied. Paste it into your Agent.", task.id) : tr("Text copied. No Task was created. Paste it into your Agent.");
  } catch (e) {
    failed = true;
    message = Number.isInteger(task.id) ? tr("Task #{0} is saved. Copy failed. Copy again or select the Task text below.", task.id) : tr("Copy failed. No Task was created. Select the Task text below.");
  }
  if (preview) { preview.message = message; preview.error = failed; preview.manualCopy = failed; }
  toast(message);
}

function handleTaskPreviewClick(e, t) {
  if (taskPreview) {
    if (t.closest('[data-task-preview-close]')) closeTaskPreview();
    else if (t.closest('[data-task-preview-copy]')) copyTask(taskPreview.key);
    else if (t.closest('[data-task-preview-assign]')) assignTask(taskPreview.key);
    else if (t.closest('[data-task-preview-check]')) checkTaskAssignment();
    else if (!taskPreview.busy && t.closest('[data-idea-restore-project]')) restoreIdeaProject(t.closest('[data-idea-restore-project]').dataset.ideaRestoreProject);
    return true;
  }
  return false;
}

function handleTaskClick(e, t) {
  const assign = t.closest('[data-task-assign]');
  if (assign) { openTaskPreview(assign.dataset.taskAssign); return true; }
  const taskStatus = t.closest('[data-task-status]');
  if (taskStatus) { changeTaskStatus(Number(taskStatus.dataset.taskId), taskStatus.dataset.taskStatus, Number(taskStatus.dataset.taskRevision)); return true; }
  const tk = t.closest('[data-task]');
  if (tk) { openTaskPreview(tk.dataset.task); return true; }
  return false;
}

export { TASK_STATUS, blockTasks, handleTaskClick, handleTaskPreviewClick, renderTaskPreview, taskBtn, taskPreviewKey, trackedTask };
