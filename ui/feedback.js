import { S, call, errText, load, render, toast } from './app.js';
import { closeIdeaRound, ideaCollection, ideaHighlight } from './ideas.js';
import { risk } from './overview.js';
import { $, dot, esc, fmtDate, openSince, shownRow, visible } from './presentation.js';

let showRated = false;  // show suggestions the owner already rated away

let noteFor = null;     // item whose "why?" field shows right after a later/down rating
let snoozeFor = null;
let snoozeDate = '';

let answering = null;   // question whose answer field is open
const drafts = {};      // unsent note and answer text per item, kept across re-renders

let round = null;       // decision round: { queue: question ids, i, stats, busy }

function rated(id) {
  const f = S.feedback[id];
  if (!f) return null;
  if (f.value === 'later' && snoozedUntil(f) <= S.today) return null;
  return f.value;
}
function addDays(date, days) {
  const d = new Date(date + 'T12:00:00Z');
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}
const snoozedUntil = f => f.snoozed_until || addDays(String(f.at).slice(0, 10), S.snooze_days || 7);
const hiddenByRating = id => !showRated && snoozeFor !== id && (rated(id) === 'down' || rated(id) === 'later');
// the owner's reply to a question: answered (with the answer as note) or done
const reply = id => { const f = id && S.feedback[id]; return f && (f.value === 'answered' || f.value === 'done') ? f : null; };
// postponed questions return on their date; until then they wait outside the list
const openQs = c => c.waiting_on_you.filter(w => !reply(w.id) && rated(w.id) !== 'later');
// how long the owner has been shown an item, from the saved card or brief versions

const questionAge = id => { const q = id && findQ(id); return q ? shownRow(q.p.item_persistence, r => r.id === id) : null; };

function findQ(id) {
  for (const p of S.projects) { const w = p.card && p.card.waiting_on_you.find(x => x.id === id); if (w) return { p, w }; }
  return null;
}

function fbButtons(id) {
  if (!id) return '';
  const cur = rated(id);
  const buttons = `<span class="fb" data-item="${esc(id)}">${[['up', 'gut'], ['later', 'später'], ['down', 'Quatsch']].map(([v, l]) => `<button type="button" data-v="${v}" aria-pressed="${cur === v}">${l}</button>`).join('')}</span>`;
  return buttons + snoozeControls(id);
}
function snoozeControls(id) {
  const until = rated(id) === 'later' ? `<span class="faint">Wieder am ${fmtDate(snoozedUntil(S.feedback[id]))} · <button class="linkish" type="button" data-snooze-open="${esc(id)}">Datum ändern</button> · <button class="linkish" type="button" data-snooze-clear="${esc(id)}">jetzt zeigen</button></span>` : '';
  const picker = snoozeFor === id ? `<div class="snooze-picker"><span>Wann möchtest du das wieder sehen?</span><div class="snooze-options"><button class="btn" type="button" data-snooze-days="1">Morgen</button><button class="btn" type="button" data-snooze-days="7">In einer Woche</button><input type="date" id="snooze-date" aria-label="Wieder anzeigen am" min="${addDays(S.today, 1)}" value="${esc(snoozeDate)}"><button class="btn primary" type="button" data-snooze-save>Datum merken</button><button class="linkish" type="button" data-snooze-cancel>Abbrechen</button></div></div>` : '';
  return until + picker;
}

function noteForm(id, label) {
  const down = S.feedback[id] && S.feedback[id].value === 'down';
  return `<div class="idea noteform${ideaHighlight === id ? ' search-hit' : ''}" data-idea-id="${esc(id)}" tabindex="-1"><div class="kind">${down ? 'Abgelehnt' : 'Später'} · ${esc(label)}</div>
    ${snoozeControls(id)}<div class="why">Magst du kurz sagen, warum? Die Agents richten künftige Vorschläge danach.</div>
    <div class="inrow"><input type="text" aria-label="Warum" data-draft="${esc(id)}" data-kind="note" maxlength="300" value="${esc(drafts[id] || '')}" placeholder="${down ? 'z. B. passt nicht zum Projekt' : 'z. B. erst nach dem Release'}">
    <button type="button" class="btn primary" data-note-save="${esc(id)}">Merken</button><button type="button" class="btn" data-note-skip>Überspringen</button></div></div>`;
}
const myNote = id => { const f = S.feedback[id]; return f && f.note && !reply(id) ? `<div class="mynote">Deine Notiz: ${esc(f.note)}</div>` : ''; };

function question(w, withSource) {
  const f = reply(w.id);
  if (!f && rated(w.id) === 'later') return `<li class="q replied"><span class="qt">${esc(w.title)}</span><div class="qr">Später · kommt am ${fmtDate(snoozedUntil(S.feedback[w.id]))} wieder · <button type="button" class="linkish" data-reply-undo="${esc(w.id)}">jetzt entscheiden</button></div></li>`;
  if (f) return `<li class="q replied"><span class="qt">${esc(w.title)}</span><div class="qr">${f.value === 'answered' ? `Deine Antwort: ${esc(f.note)}` : 'Erledigt'} · <button type="button" class="linkish" data-reply-undo="${esc(w.id)}">zurücknehmen</button></div></li>`;
  const acts = w.id && answering !== w.id ? `<span class="qa"><button type="button" data-answer="${esc(w.id)}">antworten</button><button type="button" data-qdone="${esc(w.id)}">erledigt</button><button type="button" data-qlater="${esc(w.id)}" title="Eine Woche zurückstellen">später</button></span>` : '';
  const form = answering === w.id ? `<div class="inrow"><input type="text" aria-label="Deine Antwort" data-draft="${esc(w.id)}" data-kind="answer" maxlength="600" value="${esc(drafts[w.id] || '')}" placeholder="Deine Antwort, der nächste Lauf nimmt sie in die Karte auf">
    <button type="button" class="btn primary" data-answer-save="${esc(w.id)}">Senden</button><button type="button" class="btn" data-answer-cancel>Abbrechen</button></div>` : '';
  const opts = (w.options || []).length && w.id && answering !== w.id ? `<div class="qopts">${w.options.map((o, i) => `<button type="button" class="opt${w.recommended === i + 1 ? ' rec' : ''}" data-opt="${esc(w.id)}" data-i="${i}" title="${esc(o.effect || '')}">${esc(o.answer)}${w.recommended === i + 1 ? ' <span class="rec-l">empfohlen</span>' : ''}</button>`).join('')}</div>` : '';
  const src = w.source ? (withSource ? `<div class="qsrc mono">${esc(w.source)}</div>` : '') : '';
  const age = openSince(questionAge(w.id));
  return `<li class="q"${w.source && !withSource ? ` title="Quelle: ${esc(w.source)}"` : ''}><div class="qrow"><span class="qt">${esc(w.title)}${age ? ` <span class="faint q-age">${esc(age)}</span>` : ''}</span>${acts}</div>${opts}${src}${form}</li>`;
}


// ---------- decision round: every open question, one at a time ----------
function startRound() {
  const queue = [...visible()].filter(p => p.card).sort((a, b) => risk(b) - risk(a)).flatMap(p => openQs(p.card).map(w => w.id)).filter(Boolean);
  if (!queue.length) return;
  closeIdeaRound();
  round = { queue, i: 0, stats: { answered: 0, done: 0, later: 0, skipped: 0 }, busy: false };
  renderRound();
}
const closeRound = () => { if (!round) return; round = null; $('#round-root').innerHTML = ''; };
function renderRound(keepFocus = false) {
  const root = $('#round-root');
  if (!round) { root.innerHTML = ''; return; }
  if (round.busy) return;
  const previousInput = $('#round-answer');
  const hadFocus = previousInput && document.activeElement === previousInput;
  const selection = hadFocus ? [previousInput.selectionStart, previousInput.selectionEnd] : null;
  while (round.i < round.queue.length && (!findQ(round.queue[round.i]) || reply(round.queue[round.i]) || rated(round.queue[round.i]) === 'later')) round.i++;
  const n = round.queue.length;
  let body;
  if (round.i >= n) {
    const st = round.stats;
    body = `<h3>Geschafft.</h3><p class="muted">${st.answered} beantwortet, ${st.done} erledigt, ${st.later} auf später gelegt, ${st.skipped} übersprungen. Die Agents lesen deine Antworten beim nächsten Lauf.</p>
      <div class="actions"><button class="btn primary" type="button" data-round-close>Schließen</button></div>`;
  } else {
    const { p, w } = findQ(round.queue[round.i]);
    const opts = (w.options || []).map((o, i) => `<button type="button" class="ropt${w.recommended === i + 1 ? ' rec' : ''}" data-round-opt="${i}"><span class="k mono">${i + 1}</span><span><b>${esc(o.answer)}</b>${w.recommended === i + 1 ? ' <span class="rec-l">empfohlen</span>' : ''}${o.effect ? `<span class="muted">${esc(o.effect)}</span>` : ''}</span></button>`).join('');
    const age = openSince(questionAge(w.id));
    body = `<div class="rproj">${dot(p.id)}<b>${esc(p.name)}</b>${p.card.headline ? `<span class="faint"> · ${esc(p.card.headline)}</span>` : ''}</div>
      <h3>${esc(w.title)}</h3>${age ? `<div class="faint">${esc(age)}</div>` : ''}${w.source ? `<div class="qsrc mono">${esc(w.source)}</div>` : ''}
      ${opts ? `<div class="ropts">${opts}</div>` : ''}
      <div class="inrow"><input type="text" id="round-answer" data-draft="${esc(w.id)}" data-kind="answer" value="${esc(drafts[w.id] || '')}" maxlength="600" placeholder="${opts ? 'Oder eigene Antwort' : 'Deine Antwort'}" aria-label="Deine Antwort"><button class="btn primary" type="button" data-round-send>Antworten</button></div>
      <div class="actions"><button class="btn" type="button" data-round-done title="Die Frage ist erledigt oder nicht mehr nötig">Erledigt</button><button class="btn" type="button" data-round-later title="Kommt in ${S.snooze_days || 7} Tagen wieder">Später</button><button class="btn" type="button" data-round-skip>Überspringen</button></div>`;
  }
  root.innerHTML = `<div class="scrim" data-round-close></div><div class="round" role="dialog" aria-modal="true" aria-label="Entscheidungsrunde">
    <div class="rhead"><span class="faint">Entscheidungsrunde${round.i < n ? ` · Frage ${round.i + 1} von ${n}` : ''}</span><button class="linkish" type="button" data-round-close>beenden</button></div>
    <div class="rprog"><span style="width:${(Math.min(round.i, n) / n * 100).toFixed(1)}%"></span></div>${body}</div>`;
  const input = $('#round-answer');
  if (!keepFocus || hadFocus) {
    if (input) {
      input.focus();
      if (keepFocus && selection) input.setSelectionRange(...selection);
    } else root.querySelector('[data-round-close].btn')?.focus();
  }
}
async function roundStep(value, note) {
  const active = round;
  if (!active || active.busy) return;
  const id = active.queue[active.i];
  if (!id || !findQ(id) || reply(id)) { renderRound(); return; }
  if (value === 'skip') { round.stats.skipped++; round.i++; renderRound(); return; }
  active.busy = true;
  document.querySelectorAll('#round-root button:not([data-round-close]), #round-answer').forEach(el => { el.disabled = true; });
  const saved = value === 'later' ? await react(id, 'later', undefined, undefined, addDays(S.today, S.snooze_days || 7)) : await react(id, value, note);
  // A save may finish after the owner closed this round or started another one.
  if (round !== active) return;
  active.busy = false;
  if (saved) { active.stats[value]++; active.i++; if (value !== 'later') delete drafts[id]; }
  renderRound();
}


async function react(id, value, note, msg, until) {
  const prev = S.feedback[id] ? { ...S.feedback[id] } : null;
  const idea = ideaCollection().find(entry => entry.s.id === id);
  const source = idea ? { item: { ...idea.s, kind: idea.kind }, project_id: idea.project || null, title: idea.s.title, first_seen: idea.firstSeen } : {};
  try {
    const result = await call('set_feedback', { item_id: id, value, note: note === undefined ? null : note, until: until === undefined ? null : until });
    const undoToken = typeof result.undo_token === 'string' ? result.undo_token : null;
    if (value) S.feedback[id] = { ...prev, ...source, value, at: new Date().toISOString(), note: typeof result.note === 'string' ? result.note : (note === undefined ? (prev ? prev.note : '') : note), snoozed_until: Object.hasOwn(result, 'snoozed_until') ? result.snoozed_until : (value === 'later' ? (until || (prev && prev.snoozed_until) || addDays(S.today, S.snooze_days || 7)) : null) }; else delete S.feedback[id];
    render();
    if (msg) toast(msg, undoToken ? async () => {
      noteFor = null;
      try {
        await call('undo', { token: undoToken });
        await load(false);
      } catch (e) {
        await load(false);
        toast('Rückgängig hat nicht geklappt: ' + errText(e));
      }
    } : null);
    return true;
  } catch (e) { toast('Nicht gespeichert: ' + errText(e)); return false; }
}
// a dialog renders after the page, so the last field is the one in front
const focusDraft = id => { const all = document.querySelectorAll(`[data-draft="${CSS.escape(id)}"]`); if (all.length) all[all.length - 1].focus(); };
async function setFeedback(el, v) {
  const id = el.dataset.item;
  if (v === 'later') { openSnooze(id); return; }
  const value = S.feedback[id] && S.feedback[id].value === v ? null : v;
  snoozeFor = null;
  noteFor = value === 'down' ? id : null;
  const msg = value === 'down' ? 'Unter „Abgelehnt“ gemerkt. Die Agents schlagen das nicht mehr vor.' : value === 'up' && S.prefs.style === 'ideas' ? 'Unter „Gut gefunden“ gemerkt. Noch kein Auftrag.' : null;
  if (await react(id, value, undefined, msg)) focusDraft(id); else noteFor = null;
}
function openSnooze(id) {
  snoozeFor = id; noteFor = null;
  const f = S.feedback[id];
  snoozeDate = f && rated(id) === 'later' ? snoozedUntil(f) : addDays(S.today, 7);
  render();
}
async function saveSnooze(until) {
  const id = snoozeFor;
  if (!id) return;
  if (!/^\d{4}-\d{2}-\d{2}$/.test(until || '') || until <= S.today) { toast('Wähle einen Tag nach heute.'); return; }
  snoozeFor = null; noteFor = id;
  if (await react(id, 'later', undefined, `Wieder am ${fmtDate(until)}.`, until)) focusDraft(id);
  else { noteFor = null; snoozeFor = id; render(); }
}

async function saveNote(id) {
  const text = (drafts[id] || '').trim();
  noteFor = null;
  if (!text || !S.feedback[id]) { delete drafts[id]; render(); return; }
  if (await react(id, S.feedback[id].value, text)) {
    delete drafts[id];
    toast('Gemerkt. Die Agents lesen das beim nächsten Lauf.');
  } else { noteFor = id; render(); focusDraft(id); }
}
async function saveAnswer(id) {
  const text = (drafts[id] || '').trim();
  if (!text) { focusDraft(id); return; }
  answering = null;
  if (await react(id, 'answered', text, 'Beantwortet. Der nächste Lauf nimmt deine Antwort in die Karte auf.')) delete drafts[id];
  else { answering = id; render(); focusDraft(id); }
}

function handleFeedbackClick(e, t) {
  const snoozeOpen = t.closest('[data-snooze-open]');
  if (snoozeOpen) { openSnooze(snoozeOpen.dataset.snoozeOpen); return true; }
  const snoozeDays = t.closest('[data-snooze-days]');
  if (snoozeDays) { saveSnooze(addDays(S.today, Number(snoozeDays.dataset.snoozeDays))); return true; }
  if (t.closest('[data-snooze-save]')) { saveSnooze(snoozeDate); return true; }
  if (t.closest('[data-snooze-cancel]')) { snoozeFor = null; render(); return true; }
  const snoozeClear = t.closest('[data-snooze-clear]');
  if (snoozeClear) { snoozeFor = null; noteFor = null; react(snoozeClear.dataset.snoozeClear, null, null, 'Wird wieder angezeigt.'); return true; }
  if (t.closest('[data-round-start]')) { startRound(); return true; }
  if (t.closest('[data-round-close]')) { closeRound(); return true; }
  if (round) {
    const ro = t.closest('[data-round-opt]');
    if (ro) { const q = findQ(round.queue[round.i]); const option = q && (q.w.options || [])[Number(ro.dataset.roundOpt)]; if (option) roundStep('answered', option.answer); else renderRound(); return true; }
    if (t.closest('[data-round-send]')) { const v = ($('#round-answer').value || '').trim(); if (v) roundStep('answered', v); else $('#round-answer').focus(); return true; }
    if (t.closest('[data-round-done]')) { roundStep('done', null); return true; }
    if (t.closest('[data-round-skip]')) { roundStep('skip'); return true; }
    if (t.closest('[data-round-later]')) { roundStep('later'); return true; }
  }
  const opt = t.closest('[data-opt]');
  if (opt) { const q = findQ(opt.dataset.opt); if (q) react(q.w.id, 'answered', q.w.options[Number(opt.dataset.i)].answer, 'Beantwortet. Der nächste Lauf nimmt deine Antwort in die Karte auf.'); return true; }
  const fbBtn = t.closest('.fb button');
  if (fbBtn) { e.stopPropagation(); setFeedback(fbBtn.parentElement, fbBtn.dataset.v); return true; }
  if (t.closest('[data-toggle-rated]')) { showRated = !showRated; render(); return true; }
  const ns = t.closest('[data-note-save]');
  if (ns) { saveNote(ns.dataset.noteSave); return true; }
  if (t.closest('[data-note-skip]')) { if (noteFor) delete drafts[noteFor]; noteFor = null; render(); return true; }
  const an = t.closest('[data-answer]');
  if (an) { answering = an.dataset.answer; render(); focusDraft(answering); return true; }
  const as = t.closest('[data-answer-save]');
  if (as) { saveAnswer(as.dataset.answerSave); return true; }
  if (t.closest('[data-answer-cancel]')) { answering = null; render(); return true; }
  const qd = t.closest('[data-qdone]');
  if (qd) { react(qd.dataset.qdone, 'done', null, 'Als erledigt markiert.'); return true; }
  const ql = t.closest('[data-qlater]');
  if (ql) { const until = addDays(S.today, S.snooze_days || 7); react(ql.dataset.qlater, 'later', undefined, `Zurückgestellt bis ${fmtDate(until)}.`, until); return true; }
  const ru = t.closest('[data-reply-undo]');
  if (ru) { react(ru.dataset.replyUndo, null); return true; }
  const fd = t.closest('[data-focus-done]');
  if (fd) { const id = fd.dataset.focusDone; react(id, reply(id) ? null : 'done', null); return true; }
  return false;
}

function handleFeedbackInput(e) {
  if (e.target.matches('[data-draft]')) drafts[e.target.dataset.draft] = e.target.value;
  if (e.target.dataset?.iroundNote) drafts[e.target.dataset.iroundNote] = e.target.value;
  if (e.target.id === 'snooze-date') snoozeDate = e.target.value;
  return false;
}

function handleRoundKey(e) {
  if (round) {
    if (e.key === 'Escape') { e.preventDefault(); closeRound(); return true; }
    const inInput = e.target.id === 'round-answer';
    if (inInput && e.key === 'Enter') { e.preventDefault(); const v = e.target.value.trim(); if (v) roundStep('answered', v); return true; }
    if (!inInput) {
      const k = Number(e.key), q = findQ(round.queue[round.i] || '');
      if (k >= 1 && q && q.w.options && q.w.options[k - 1]) { e.preventDefault(); roundStep('answered', q.w.options[k - 1].answer); }
    }
    return true;
  }
  return false;
}

function handleFeedbackKey(e) {
  const draft = e.target.closest && e.target.closest('[data-draft]');
  if (draft && (e.key === 'Enter' || e.key === 'Escape')) {
    e.preventDefault();
    const id = draft.dataset.draft, note = draft.dataset.kind === 'note';
    if (e.key === 'Enter') { if (note) saveNote(id); else saveAnswer(id); }
    else { if (note) { delete drafts[id]; noteFor = null; } else answering = null; render(); }
    return true;
  }
  return false;
}

export { closeRound, drafts, fbButtons, handleFeedbackClick, handleFeedbackInput, handleFeedbackKey, handleRoundKey, hiddenByRating, myNote, noteFor, noteForm, openQs, question, questionAge, rated, react, renderRound, reply, round, showRated, snoozeFor, snoozedUntil };
