import { S } from './app.js';

// ten hues spread around the wheel, readable on the light and the dark background
const PALETTE = ['#e8870a', '#2f7fe0', '#2a9d5c', '#d43f8d', '#8a5cd6', '#d9443a', '#14a3b8', '#c9a10c', '#8d6e55', '#6b7a90'];
const PHASE = { active: 'aktiv', cooling: 'kühlt ab', dormant: 'schläft', done: 'fertig', maintenance: 'Wartung' };
const STATUS = { active: 'aktiv', parked: 'geparkt', done: 'fertig', archived: 'archiviert' };
const KIND = { worktree: 'Worktree', branch: 'Branch', uncommitted: 'nicht committet', unpushed: 'nicht gepusht', ci: 'CI', flagged: 'FLAGGED', todo: 'TODO', plan: 'Plan', other: 'Sonstiges' };
const TYPE = { next: 'Nächster Schritt', feature: 'Feature', cleanup: 'Aufräumen', park: 'Parken' };
const CI = { green: ['grün', 'good'], red: ['rot', 'bad'], running: ['läuft', ''], none: ['keins', ''], unknown: ['unbekannt', ''], skipped: ['—', ''] };
const VERDICT = { park: ['parken', 'warn'], revive: ['zurückholen', 'good'], finish: ['fertig machen', 'good'], archive: ['archivieren', 'bad'] };
// the owner intent that takes over a park/archive/revive verdict
const VERDICT_STATUS = { park: 'parked', archive: 'archived', revive: 'active' };
const MOD_TITLES = {
  focus: 'Heute zuerst', projects: 'Deine Projekte', waiting: 'Wartet auf dich', safety: 'Sicherheitslage', recent: 'Was passiert ist',
  across: 'Quer durch alle Projekte', ideas: 'Ideen', pulse: 'Puls', kpis: 'Lage auf einen Blick', cards: 'Projekte',
};


const openFolds = new Set();
const foldAttrs = id => `data-fold="${esc(id)}"${openFolds.has(id) ? ' open' : ''}`;

const $ = sel => document.querySelector(sel);
const esc = s => String(s == null ? '' : s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const dayMs = 864e5;
const todayDate = () => new Date(S.today + 'T12:00:00');
const daysSince = d => Math.round((todayDate() - new Date(String(d).slice(0, 10) + 'T12:00:00')) / dayMs);
const ago = d => { if (!d) return 'nie'; const n = daysSince(d); return n <= 0 ? 'heute' : n === 1 ? 'gestern' : `vor ${n} Tagen`; };
const minutesAgo = iso => Math.max(0, Math.round((Date.now() - Date.parse(iso)) / 60000));
const fmtDate = d => { const [y, m, dd] = String(d).slice(0, 10).split('-'); return `${dd}.${m}.${y}`; };
const fmtShort = d => { const [, m, dd] = String(d).slice(0, 10).split('-'); return `${dd}.${m}.`; };
const num = n => Number(n || 0).toLocaleString('de-DE');
const hash = s => { let h = 0; for (const c of s) h = (h * 31 + c.charCodeAt(0)) >>> 0; return h; };
// Each project keeps its hashed color unless an earlier project holds it; then it
// takes the next free one, so up to ten projects never share a color.
let colors = { key: null, map: new Map() };
function color(id) {
  const ids = ((S && S.projects) || []).map(p => p.id), key = ids.join(' ');
  if (colors.key !== key) {
    const map = new Map(), used = new Set();
    for (const pid of ids) {
      let slot = hash(pid) % PALETTE.length;
      for (let i = 0; i < PALETTE.length && used.has(slot); i++) slot = (slot + 1) % PALETTE.length;
      used.add(slot); map.set(pid, PALETTE[slot]);
    }
    colors = { key, map };
  }
  return colors.map.get(id) || PALETTE[hash(id) % PALETTE.length];
}
const dot = id => `<span class="dot" style="background:${color(id)}"></span>`;
const byId = id => S.projects.find(p => p.id === id);
const plink = id => { const p = byId(id); return p ? `<button class="plink" type="button" data-open="${esc(id)}">${dot(id)}${esc(p.name)}</button>` : esc(id); };

const visible = () => S.projects.filter(p => p.status !== 'archived');
const lastWork = p => (p.facts && p.facts.last_work) || (p.card_updated_at || '').slice(0, 10) || null;
const byLastWork = list => [...list].sort((a, b) => (lastWork(b) || '').localeCompare(lastWork(a) || ''));

const shownRow = (rows, match) => (rows || []).find(match) || null;

const days = n => `${num(n)} ${n === 1 ? 'Tag' : 'Tagen'}`;
const openSince = row => row && row.days >= 3 ? `seit ${days(row.days)} offen` : '';
// a measured risk that has lasted: since when and whether it grows
const streak = (p, kind) => shownRow(p && p.risk_streaks, s => s.kind === kind);
const streakText = s => s ? `${s.at_least ? 'seit mindestens' : 'seit'} ${days(s.days)}${s.trend === 'growing' ? ', wächst' : s.trend === 'shrinking' ? ', schrumpft' : ''}` : '';

// ---------- building blocks ----------
// Keep the stored preference compatible; formerly hidden sections now stay reachable.
const collapsedMods = () => new Set(S.prefs.hidden_modules || []);
function mod(id, title, sub, body) {
  MOD_TITLES[id] = title;
  if (!body) return '';
  const collapsed = collapsedMods().has(id), action = collapsed ? 'ausklappen' : 'einklappen';
  return `<section class="mod${collapsed ? ' is-collapsed' : ''}" data-mod="${id}"><div class="mod-head"><div><h2>${esc(title)}</h2>${sub && !collapsed ? `<div class="sub">${esc(sub)}</div>` : ''}</div><button class="mod-toggle" type="button" data-mod-toggle="${id}" aria-expanded="${!collapsed}" aria-controls="mod-${id}-body" aria-label="${esc(title)} ${action}"><span aria-hidden="true">${collapsed ? '▸' : '▾'}</span> ${action}</button></div><div class="mod-body" id="mod-${id}-body"${collapsed ? ' hidden' : ''}>${body}</div></section>`;
}
const dmod = (title, body, sub) => body ? `<section class="mod"><div class="mod-head"><div><h2>${esc(title)}</h2>${sub ? `<div class="sub">${esc(sub)}</div>` : ''}</div></div>${body}</section>` : '';

function spark(weekly, col, h = 38) {
  const w = 260, n = weekly.length, max = Math.max(1, ...weekly), pad = 3;
  const x = i => pad + (i * (w - 2 * pad)) / (n - 1);
  const y = v => h - pad - (v / max) * (h - 2 * pad);
  const pts = weekly.map((v, i) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`);
  const area = `M${x(0)},${h - pad} L${pts.join(' L')} L${x(n - 1)},${h - pad} Z`;
  return `<svg class="spark" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none" role="img" aria-label="Commits pro Woche über 26 Wochen, höchstens ${max}">
    <line x1="${pad}" x2="${w - pad}" y1="${h - pad}" y2="${h - pad}" stroke="var(--line)" stroke-width="1"/>
    <path d="${area}" fill="${col}" fill-opacity="0.18"/>
    <polyline points="${pts.join(' ')}" fill="none" stroke="${col}" stroke-width="1.6" vector-effect="non-scaling-stroke"/>
    <circle cx="${x(n - 1)}" cy="${y(weekly[n - 1])}" r="2.6" fill="${col}"/></svg>`;
}
const weekly = p => (p.facts && p.facts.commits && p.facts.commits.weekly) || new Array(26).fill(0);


const bytes = n => n >= 1e9 ? `${(n / 1e9).toLocaleString('de-DE', { maximumFractionDigits: 1 })} GB` : `${Math.round(n / 1e6).toLocaleString('de-DE')} MB`;

function handleFoldToggle(e) {
  const id = e.target.dataset?.fold;
  if (id) { if (e.target.open) openFolds.add(id); else openFolds.delete(id); }
  return false;
}

export { $, CI, KIND, MOD_TITLES, PALETTE, PHASE, STATUS, TYPE, VERDICT, VERDICT_STATUS, ago, byId, byLastWork, bytes, collapsedMods, color, days, daysSince, dmod, dot, esc, fmtDate, fmtShort, foldAttrs, handleFoldToggle, lastWork, minutesAgo, mod, num, openFolds, openSince, plink, shownRow, spark, streak, streakText, visible, weekly };
