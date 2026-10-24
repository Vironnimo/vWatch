import german from './messages-de.js';
import errors from './errors-de.js';

// English messages live at their call sites. Interpolation replaces slots once,
// so stored text containing {0}, markup or translation keys remains data.
let language = 'en';
const locale = () => language === 'de' ? 'de-DE' : 'en-US';
function setLanguage(value) {
  const next = value === 'de' ? 'de' : 'en', changed = next !== language;
  language = next;
  return changed;
}
function tr(message, ...values) {
  const english = message.split('\u0004')[0];
  const text = language === 'de' ? Object.hasOwn(german, message) ? german[message] : Object.hasOwn(errors, message) ? errors[message] : english : english;
  return text.replace(/\{(\d+)\}/g, (slot, index) => index < values.length ? String(values[index]) : slot);
}

// Match only known service diagnostics; captured paths/IDs remain literal data.
const errorPatterns = Object.keys(errors).filter(message => /\{\d+\}/.test(message)).map(message => ({
  message,
  pattern: new RegExp('^' + message.split(/\{\d+\}/).map(part => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('([\\s\\S]+?)') + '$'),
}));
function errorText(message) {
  if (language === 'de' && !Object.hasOwn(errors, message)) {
    for (const { message: template, pattern } of errorPatterns) {
      const found = pattern.exec(message);
      if (found) return tr(template, ...found.slice(1));
    }
  }
  return tr(message);
}

// Label tables are evaluated when read, including after a language switch.
function localized(read) {
  const overrides = { en: {}, de: {} };
  const tables = {};
  const current = () => tables[language] ??= read();
  return new Proxy({}, {
    get: (_target, key) => {
      const table = current();
      const value = Array.isArray(table) ? table : { ...table, ...overrides[language] };
      return typeof value[key] === 'function' ? value[key].bind(value) : value[key];
    },
    set: (_target, key, value) => { overrides[language][key] = value; return true; },
    ownKeys: () => Reflect.ownKeys({ ...current(), ...overrides[language] }),
    getOwnPropertyDescriptor: () => ({ enumerable: true, configurable: true }),
  });
}

export { errorText, locale, localized, setLanguage, tr };
