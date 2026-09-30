//! The two documents agents write: a project card and the portfolio brief.
//!
//! One schema description drives validation and the JSON schema that
//! `lookout schema` prints. Validation is lenient about extras: unknown fields
//! are dropped and reported as warnings, while missing or malformed fields fail
//! with their path. Items the owner reacts to (suggestions and questions on a
//! card; focus items, patterns, synergies, app ideas and verdicts in the brief)
//! carry a stable `id`; new items get one, and an agent keeps an item's identity
//! by sending its previous id again.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Card,
    Portfolio,
}

enum Ty {
    /// Text of `min..=max` characters and, when set, at most `words` words.
    Str { min: usize, max: usize, words: Option<usize> },
    Date,
    Enum(&'static [&'static str]),
    Int { min: i64, max: i64 },
    Id,
    List(&'static Ty, Option<usize>),
    Obj(&'static [Field]),
    Nullable(&'static Ty),
}

enum Default {
    Required,
    Empty,
    Null,
    Text(&'static str),
    /// An object whose fields all have defaults.
    Filled,
}

struct Field {
    name: &'static str,
    ty: Ty,
    default: Default,
}

const fn f(name: &'static str, ty: Ty, default: Default) -> Field {
    Field { name, ty, default }
}

const SHORT: Ty = Ty::Str { min: 1, max: 160, words: None };
const TEXT: Ty = Ty::Str { min: 0, max: 600, words: None };
/// Required short text of at most `n` words: the page has room for exactly that.
const fn words(n: usize) -> Ty {
    Ty::Str { min: 1, max: 600, words: Some(n) }
}
/// Optional text of at most `n` words; "one sentence" fields allow 30.
const fn sentence(n: usize) -> Ty {
    Ty::Str { min: 0, max: 600, words: Some(n) }
}
const SENTENCE: Ty = sentence(30);
/// A one-sentence reason of at most 25 words.
const WHY: Ty = Ty::Str { min: 1, max: 600, words: Some(25) };
const EFFORT: &[&str] = &["15min", "1h", "half-day", "day+"];
const SIZE: &[&str] = &["S", "M", "L"];
/// A command, URL, shortcut or path the owner types or clicks.
const COMMAND: Ty = Ty::Str { min: 1, max: 600, words: None };
use Default::{Empty, Filled, Null, Required, Text};

const CHANGE: &[Field] = &[f("date", Ty::Date, Required), f("title", words(8), Required), f("detail", SENTENCE, Text(""))];
const MILESTONE: &[Field] = &[f("date", Ty::Date, Required), f("title", words(10), Required)];
const OPEN_END: &[Field] = &[
    f("kind", Ty::Enum(&["worktree", "branch", "uncommitted", "unpushed", "ci", "flagged", "todo", "plan", "other"]), Text("other")),
    f("title", words(10), Required),
    f("detail", SENTENCE, Text("")),
    f("source", TEXT, Text("")),
    f("age_days", Ty::Nullable(&Ty::Int { min: 0, max: 100_000 }), Null),
];
const QUESTION: &[Field] = &[f("id", Ty::Nullable(&Ty::Id), Null), f("title", words(12), Required), f("source", TEXT, Text(""))];
const VISION: &[Field] = &[
    f("statement", Ty::Nullable(&SENTENCE), Null),
    f("progress_note", Ty::Nullable(&SENTENCE), Null),
    f("progress_pct", Ty::Nullable(&Ty::Int { min: 0, max: 100 }), Null),
];
/// How an agent handed the task would know it is finished.
const DONE_WHEN: Ty = sentence(20);
const REENTRY: &[Field] =
    &[f("next_step", words(20), Required), f("effort", Ty::Enum(EFFORT), Required), f("why", SENTENCE, Text("")), f("done_when", DONE_WHEN, Text(""))];
const RUN: &[Field] = &[f("what", words(6), Required), f("how", COMMAND, Required)];
const PLACE: &[Field] = &[f("path", COMMAND, Required), f("what", words(12), Required)];
const READ: &[Field] = &[f("path", COMMAND, Required), f("why", words(12), Required)];
const INSTALLED: &[Field] = &[f("version", words(4), Required), f("path", COMMAND, Required)];
const TERM: &[Field] = &[f("term", words(4), Required), f("meaning", words(20), Required)];
/// What someone returning after months needs to find their way around.
const HANDBOOK: &[Field] = &[
    f("run", Ty::List(&Ty::Obj(RUN), Some(6)), Empty),
    f("installed", Ty::Nullable(&Ty::Obj(INSTALLED)), Null),
    f("architecture", sentence(60), Text("")),
    f("places", Ty::List(&Ty::Obj(PLACE), Some(10)), Empty),
    f("read_first", Ty::List(&Ty::Obj(READ), Some(5)), Empty),
    f("needs", Ty::List(&sentence(15), Some(8)), Empty),
    f("terms", Ty::List(&Ty::Obj(TERM), Some(8)), Empty),
];
const SUGGESTION: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("type", Ty::Enum(&["next", "feature", "cleanup", "park"]), Required),
    f("title", words(8), Required),
    f("why", WHY, Required),
    f("evidence", TEXT, Text("")),
    f("effort", Ty::Enum(SIZE), Required),
    f("done_when", DONE_WHEN, Text("")),
];
const CARD: &[Field] = &[
    f("one_liner", words(15), Required),
    f("stack", sentence(25), Text("")),
    f("phase", Ty::Enum(&["active", "cooling", "dormant", "maintenance", "done"]), Required),
    f("phase_evidence", SENTENCE, Text("")),
    f("headline", words(10), Required),
    f("personality", SENTENCE, Text("")),
    f("recent_changes", Ty::List(&Ty::Obj(CHANGE), None), Empty),
    f("milestones", Ty::List(&Ty::Obj(MILESTONE), None), Empty),
    f("features", Ty::List(&words(8), None), Empty),
    f("open_ends", Ty::List(&Ty::Obj(OPEN_END), None), Empty),
    f("waiting_on_you", Ty::List(&Ty::Obj(QUESTION), None), Empty),
    f("vision", Ty::Obj(VISION), Filled),
    f("reentry", Ty::Obj(REENTRY), Required),
    f("health_notes", Ty::List(&SENTENCE, None), Empty),
    f("decisions", Ty::List(&sentence(20), None), Empty),
    f("suggestions", Ty::List(&Ty::Obj(SUGGESTION), None), Empty),
    f("cross_project_hints", Ty::List(&SENTENCE, None), Empty),
    f("handbook", Ty::Obj(HANDBOOK), Filled),
];

const FOCUS: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("project", SHORT, Required),
    f("title", words(8), Required),
    f("why", WHY, Required),
    f("effort", Ty::Enum(EFFORT), Required),
    f("done_when", DONE_WHEN, Text("")),
];
const PATTERN: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("title", words(8), Required),
    f("why", WHY, Required),
    f("projects", Ty::List(&SHORT, None), Empty),
    f("evidence", TEXT, Text("")),
    f("severity", Ty::Enum(&["info", "warn", "risk"]), Text("info")),
];
const SYNERGY: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("title", words(8), Required),
    f("why", WHY, Required),
    f("projects", Ty::List(&SHORT, None), Empty),
    f("evidence", TEXT, Text("")),
    f("effort", Ty::Enum(SIZE), Required),
];
const APP_IDEA: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("title", words(8), Required),
    f("pitch", WHY, Required),
    f("because", WHY, Required),
    f("effort", Ty::Enum(SIZE), Required),
];
const VERDICT: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("project", SHORT, Required),
    f("verdict", Ty::Enum(&["park", "revive", "finish", "archive"]), Required),
    f("why", WHY, Required),
];
const PORTFOLIO: &[Field] = &[
    f("greeting", Ty::Str { min: 1, max: 600, words: Some(30) }, Required),
    f("focus", Ty::List(&Ty::Obj(FOCUS), Some(3)), Empty),
    f("patterns", Ty::List(&Ty::Obj(PATTERN), None), Empty),
    f("synergies", Ty::List(&Ty::Obj(SYNERGY), None), Empty),
    f("new_apps", Ty::List(&Ty::Obj(APP_IDEA), None), Empty),
    f("park_or_revive", Ty::List(&Ty::Obj(VERDICT), None), Empty),
    f("attention_note", SENTENCE, Text("")),
];

impl Kind {
    fn fields(self) -> &'static [Field] {
        match self {
            Kind::Card => CARD,
            Kind::Portfolio => PORTFOLIO,
        }
    }

    /// The lists whose items carry ids: (list, prefix of generated ids, item kind).
    /// The top-level fields of the document.
    pub fn field_names(self) -> impl Iterator<Item = &'static str> {
        self.fields().iter().map(|f| f.name)
    }

    pub fn items(self) -> &'static [(&'static str, &'static str, &'static str)] {
        match self {
            Kind::Card => &[("suggestions", "sg", "suggestion"), ("waiting_on_you", "q", "question")],
            Kind::Portfolio => &[
                ("focus", "fc", "focus"),
                ("patterns", "pt", "pattern"),
                ("synergies", "sy", "synergy"),
                ("new_apps", "ap", "new_app"),
                ("park_or_revive", "vd", "verdict"),
            ],
        }
    }

    fn title(self) -> &'static str {
        match self {
            Kind::Card => "Card",
            Kind::Portfolio => "Portfolio",
        }
    }
}

/// Validate a document and assign ids to rated items. Returns (clean document, warnings)
/// or the list of problems as "path: message". Word limits are not enforced on the
/// top-level fields in `kept`, text carried over unchanged from the previous version that
/// may predate a limit; such text only earns a warning.
pub fn validate(kind: Kind, data: &Value, kept: &[String]) -> Result<(Value, Vec<String>), Vec<String>> {
    if !data.is_object() {
        return Err(vec!["(document): the document must be a JSON object".into()]);
    }
    let mut ctx = Ctx { problems: Vec::new(), unknown: Vec::new(), over: Vec::new(), kept, enforce_words: true };
    let clean = check_obj(kind.fields(), data, "", &mut ctx);
    if !ctx.problems.is_empty() {
        return Err(ctx.problems);
    }
    let mut clean = clean.expect("a valid object yields a value");
    let mut seen = HashSet::new();
    for (list, prefix, _) in kind.items() {
        if let Some(Value::Array(items)) = clean.get_mut(*list) {
            for item in items {
                let id = item.get("id").and_then(Value::as_str).map(str::to_owned);
                let id = match id {
                    Some(id) if !seen.contains(&id) => id,
                    _ => new_id(prefix),
                };
                seen.insert(id.clone());
                item["id"] = Value::String(id);
            }
        }
    }
    let warnings = ctx
        .unknown
        .iter()
        .map(|p| format!("ignored unknown field: {p}"))
        .chain(ctx.over.iter().map(|p| format!("kept from the previous version although over its word limit: {p}; shorten it when you next send that field")))
        .collect();
    Ok((clean, warnings))
}

struct Ctx<'a> {
    problems: Vec<String>,
    unknown: Vec<String>,
    /// Kept texts over their word limit, as "path (N of M words)".
    over: Vec<String>,
    kept: &'a [String],
    enforce_words: bool,
}

fn fail(ctx: &mut Ctx, path: &str, msg: String) -> Option<Value> {
    ctx.problems.push(format!("{path}: {msg}"));
    None
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() { name.to_owned() } else { format!("{path}.{name}") }
}

fn check_obj(fields: &[Field], value: &Value, path: &str, ctx: &mut Ctx) -> Option<Value> {
    let Some(map) = value.as_object() else {
        return fail(ctx, if path.is_empty() { "(document)" } else { path }, "Input should be an object".into());
    };
    for key in map.keys() {
        if !fields.iter().any(|f| f.name == key) {
            ctx.unknown.push(join(path, key));
        }
    }
    let mut out = Map::new();
    for field in fields {
        let here = join(path, field.name);
        let outer = ctx.enforce_words;
        if path.is_empty() {
            ctx.enforce_words = !ctx.kept.iter().any(|k| k == field.name);
        }
        let given = map.get(field.name).filter(|v| !v.is_null() || matches!(field.ty, Ty::Nullable(_)));
        let result = match given {
            Some(v) => check(&field.ty, v, &here, ctx),
            None => match field.default {
                Required => fail(ctx, &here, "Field required".into()),
                Empty => Some(json!([])),
                Null => Some(Value::Null),
                Text(s) => Some(json!(s)),
                Filled => check(&field.ty, &json!({}), &here, ctx),
            },
        };
        ctx.enforce_words = outer;
        if let Some(v) = result {
            out.insert(field.name.to_owned(), v);
        }
    }
    Some(Value::Object(out))
}

fn check(ty: &Ty, value: &Value, path: &str, ctx: &mut Ctx) -> Option<Value> {
    match ty {
        Ty::Nullable(inner) => {
            if value.is_null() { Some(Value::Null) } else { check(inner, value, path, ctx) }
        }
        Ty::Str { min, max, words } => match value.as_str() {
            None => fail(ctx, path, "Input should be a string".into()),
            Some(s) if s.chars().count() < *min => fail(ctx, path, format!("String should have at least {min} character")),
            Some(s) if s.chars().count() > *max => fail(ctx, path, format!("String should have at most {max} characters")),
            Some(s) if words.is_some_and(|w| word_count(s) > w) => {
                let (limit, n) = (words.unwrap_or_default(), word_count(s));
                if ctx.enforce_words {
                    fail(ctx, path, format!("Text should have at most {limit} words, has {n}"))
                } else {
                    ctx.over.push(format!("{path} ({n} of {limit} words)"));
                    Some(json!(s))
                }
            }
            Some(s) => Some(json!(s)),
        },
        Ty::Date => match value.as_str() {
            Some(s) if s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() => Some(json!(s)),
            _ => fail(ctx, path, "Input should be a date as YYYY-MM-DD".into()),
        },
        Ty::Id => match value.as_str() {
            Some(s) if valid_item_id(s) => Some(json!(s)),
            _ => fail(ctx, path, "Input should be an id of 3-64 characters: lowercase letters, digits, ':', '_' or '-'".into()),
        },
        Ty::Enum(options) => match value.as_str() {
            Some(s) if options.contains(&s) => Some(json!(s)),
            _ => {
                let quoted: Vec<String> = options.iter().map(|o| format!("'{o}'")).collect();
                fail(ctx, path, format!("Input should be one of {}", quoted.join(", ")))
            }
        },
        Ty::Int { min, max } => {
            let n = value.as_i64().or_else(|| value.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64));
            match n {
                None => fail(ctx, path, "Input should be a whole number".into()),
                Some(n) if n < *min || n > *max => fail(ctx, path, format!("Input should be between {min} and {max}")),
                Some(n) => Some(json!(n)),
            }
        }
        Ty::List(item, max) => {
            let Some(items) = value.as_array() else { return fail(ctx, path, "Input should be a list".into()) };
            if let Some(max) = max {
                if items.len() > *max {
                    return fail(ctx, path, format!("List should have at most {max} items"));
                }
            }
            let before = ctx.problems.len();
            let out: Vec<Value> = items.iter().enumerate().filter_map(|(i, v)| check(item, v, &format!("{path}[{i}]"), ctx)).collect();
            (ctx.problems.len() == before).then(|| Value::Array(out))
        }
        Ty::Obj(fields) => {
            let before = ctx.problems.len();
            let out = check_obj(fields, value, path, ctx);
            if ctx.problems.len() == before { out } else { None }
        }
    }
}

fn word_count(s: &str) -> usize {
    s.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count()
}

fn valid_item_id(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=64).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b':' | b'_' | b'-'))
}

fn new_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let digest = Sha1::digest(format!("{nanos}:{n}:{}", std::process::id()).as_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{prefix}-{hex}")
}

/// The JSON schema of a document, generated from the same description validation uses.
pub fn json_schema(kind: Kind) -> Value {
    let mut schema = obj_schema(kind.fields());
    schema["title"] = json!(kind.title());
    schema
}

fn obj_schema(fields: &[Field]) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for field in fields {
        let mut s = ty_schema(&field.ty);
        match field.default {
            Required => required.push(json!(field.name)),
            Empty => s["default"] = json!([]),
            Null => s["default"] = Value::Null,
            Text(t) => s["default"] = json!(t),
            Filled => {}
        }
        props.insert(field.name.to_owned(), s);
    }
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

fn ty_schema(ty: &Ty) -> Value {
    match ty {
        Ty::Str { min, max, words } => {
            let mut s = json!({"type": "string", "maxLength": max});
            if *min > 0 {
                s["minLength"] = json!(min);
            }
            if let Some(w) = words {
                s["description"] = json!(format!("At most {w} words."));
            }
            s
        }
        Ty::Date => json!({"type": "string", "format": "date", "pattern": "^\\d{4}-\\d{2}-\\d{2}$"}),
        Ty::Id => json!({"type": "string", "pattern": "^[a-z0-9][a-z0-9:_-]{2,63}$"}),
        Ty::Enum(options) => json!({"type": "string", "enum": options}),
        Ty::Int { min, max } => json!({"type": "integer", "minimum": min, "maximum": max}),
        Ty::List(item, max) => {
            let mut s = json!({"type": "array", "items": ty_schema(item)});
            if let Some(max) = max {
                s["maxItems"] = json!(max);
            }
            s
        }
        Ty::Obj(fields) => obj_schema(fields),
        Ty::Nullable(inner) => json!({"anyOf": [ty_schema(inner), {"type": "null"}]}),
    }
}
