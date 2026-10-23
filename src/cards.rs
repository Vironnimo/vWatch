//! The two documents agents write: a project card and the portfolio brief.
//!
//! One schema description drives validation and the JSON schema that
//! `watchtower --json schema` prints. Validation is lenient about extras: unknown fields
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
    Str {
        min: usize,
        max: usize,
        words: Option<usize>,
    },
    Date,
    Week,
    Enum(&'static [&'static str]),
    Int {
        min: i64,
        max: i64,
    },
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

const SHORT: Ty = Ty::Str {
    min: 1,
    max: 160,
    words: None,
};
const TEXT: Ty = Ty::Str {
    min: 0,
    max: 600,
    words: None,
};
/// Required short text of at most `n` words: the page has room for exactly that.
const fn words(n: usize) -> Ty {
    Ty::Str {
        min: 1,
        max: 600,
        words: Some(n),
    }
}
/// Optional text of at most `n` words; "one sentence" fields allow 30.
const fn sentence(n: usize) -> Ty {
    Ty::Str {
        min: 0,
        max: 600,
        words: Some(n),
    }
}
const SENTENCE: Ty = sentence(30);
/// A one-sentence reason of at most 25 words.
const WHY: Ty = Ty::Str {
    min: 1,
    max: 600,
    words: Some(25),
};
/// A command, URL, shortcut or path the owner types or clicks.
const COMMAND: Ty = Ty::Str {
    min: 1,
    max: 600,
    words: None,
};
use Default::{Empty, Filled, Null, Required, Text};

const CHANGE: &[Field] = &[
    f("date", Ty::Date, Required),
    f("title", words(8), Required),
    f("detail", SENTENCE, Text("")),
];
const MILESTONE: &[Field] = &[
    f("date", Ty::Date, Required),
    f("title", words(10), Required),
];
const OPEN_END: &[Field] = &[
    f(
        "kind",
        Ty::Enum(&[
            "worktree",
            "branch",
            "uncommitted",
            "unpushed",
            "ci",
            "flagged",
            "todo",
            "plan",
            "other",
        ]),
        Text("other"),
    ),
    f("title", words(10), Required),
    f("detail", SENTENCE, Text("")),
    f("source", TEXT, Text("")),
    f(
        "age_days",
        Ty::Nullable(&Ty::Int {
            min: 0,
            max: 100_000,
        }),
        Null,
    ),
];
const OPTION: &[Field] = &[
    f("answer", words(8), Required),
    f("effect", sentence(20), Text("")),
];
const QUESTION: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("title", words(12), Required),
    f("source", TEXT, Text("")),
    f("options", Ty::List(&Ty::Obj(OPTION), Some(3)), Empty),
    // The 1-based number of the option the agent recommends, when one is clearly better.
    f(
        "recommended",
        Ty::Nullable(&Ty::Int { min: 1, max: 3 }),
        Null,
    ),
];
const VISION: &[Field] = &[
    f("statement", Ty::Nullable(&SENTENCE), Null),
    f("progress_note", Ty::Nullable(&SENTENCE), Null),
    f(
        "progress_pct",
        Ty::Nullable(&Ty::Int { min: 0, max: 100 }),
        Null,
    ),
];
/// How an agent handed the task would know it is finished.
const DONE_WHEN: Ty = sentence(20);
/// How the owner actually uses the project today, as found in its traces.
const USAGE: &[Field] = &[
    f("summary", Ty::Nullable(&SENTENCE), Null),
    f("last_used", Ty::Nullable(&Ty::Date), Null),
    f("evidence", TEXT, Text("")),
];
const REENTRY: &[Field] = &[
    f("next_step", words(20), Required),
    f("why", SENTENCE, Text("")),
    f("done_when", DONE_WHEN, Text("")),
];
const RUN: &[Field] = &[
    f("what", words(8), Required),
    f("how", COMMAND, Required),
    f("folder", TEXT, Text("")),
];
const PLACE: &[Field] = &[f("path", COMMAND, Required), f("what", words(12), Required)];
const READ: &[Field] = &[f("path", COMMAND, Required), f("why", words(12), Required)];
const INSTALLED: &[Field] = &[
    f("version", words(4), Required),
    f("path", COMMAND, Required),
    f("open", sentence(8), Text("")),
];
const TERM: &[Field] = &[
    f("term", words(4), Required),
    f("meaning", words(20), Required),
];
/// What someone returning after months needs to find their way around.
const HANDBOOK: &[Field] = &[
    f("run", Ty::List(&Ty::Obj(RUN), Some(8)), Empty),
    f("installed", Ty::Nullable(&Ty::Obj(INSTALLED)), Null),
    f("architecture", sentence(60), Text("")),
    f("places", Ty::List(&Ty::Obj(PLACE), Some(10)), Empty),
    f("read_first", Ty::List(&Ty::Obj(READ), Some(5)), Empty),
    f("needs", Ty::List(&sentence(15), Some(8)), Empty),
    f("terms", Ty::List(&Ty::Obj(TERM), Some(8)), Empty),
];
const SUGGESTION: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f(
        "category",
        Ty::Enum(&["idea", "recommendation"]),
        Text("recommendation"),
    ),
    f(
        "type",
        Ty::Enum(&["next", "feature", "cleanup", "park"]),
        Required,
    ),
    f("title", words(8), Required),
    f("why", WHY, Required),
    f("evidence", TEXT, Text("")),
    f("done_when", DONE_WHEN, Text("")),
];
const CARD: &[Field] = &[
    f("one_liner", words(15), Required),
    f("stack", sentence(25), Text("")),
    f(
        "phase",
        Ty::Enum(&["active", "cooling", "dormant", "maintenance", "done"]),
        Required,
    ),
    f("phase_evidence", SENTENCE, Text("")),
    f("headline", words(10), Required),
    f("personality", SENTENCE, Text("")),
    f("recent_changes", Ty::List(&Ty::Obj(CHANGE), None), Empty),
    f("milestones", Ty::List(&Ty::Obj(MILESTONE), None), Empty),
    f("features", Ty::List(&words(8), None), Empty),
    f("usage", Ty::Obj(USAGE), Filled),
    f("open_ends", Ty::List(&Ty::Obj(OPEN_END), None), Empty),
    f(
        "waiting_on_you",
        Ty::List(&Ty::Obj(QUESTION), Some(3)),
        Empty,
    ),
    f("vision", Ty::Obj(VISION), Filled),
    f("reentry", Ty::Obj(REENTRY), Required),
    f("health_notes", Ty::List(&SENTENCE, None), Empty),
    f("decisions", Ty::List(&sentence(20), None), Empty),
    f(
        "suggestions",
        Ty::List(&Ty::Obj(SUGGESTION), Some(8)),
        Empty,
    ),
    f("cross_project_hints", Ty::List(&SENTENCE, None), Empty),
    f("handbook", Ty::Obj(HANDBOOK), Filled),
];

const FOCUS: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("project", SHORT, Required),
    f("title", words(8), Required),
    f("why", WHY, Required),
    f("done_when", DONE_WHEN, Text("")),
];
const PATTERN: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("title", words(8), Required),
    f("why", WHY, Required),
    f("projects", Ty::List(&SHORT, None), Empty),
    f("evidence", TEXT, Text("")),
    f(
        "severity",
        Ty::Enum(&["info", "warn", "risk"]),
        Text("info"),
    ),
];
const SYNERGY: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f(
        "category",
        Ty::Enum(&["idea", "recommendation"]),
        Text("recommendation"),
    ),
    f("title", words(8), Required),
    f("why", WHY, Required),
    f("projects", Ty::List(&SHORT, None), Empty),
    f("evidence", TEXT, Text("")),
];
const APP_IDEA: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("title", words(8), Required),
    f("pitch", WHY, Required),
    f("because", WHY, Required),
];
const VERDICT: &[Field] = &[
    f("id", Ty::Nullable(&Ty::Id), Null),
    f("project", SHORT, Required),
    f(
        "verdict",
        Ty::Enum(&["park", "revive", "finish", "archive"]),
        Required,
    ),
    f("why", WHY, Required),
];
/// An agent's assessment of an idea the owner parked.
const IDEA_CHECK: &[Field] = &[
    f(
        "idea",
        Ty::Int {
            min: 1,
            max: i64::MAX,
        },
        Required,
    ),
    f(
        "verdict",
        Ty::Enum(&["build", "fold_in", "later", "drop"]),
        Required,
    ),
    f("fits", Ty::List(&SHORT, None), Empty),
    f("why", WHY, Required),
];
/// What the past calendar week brought, written by the first brief of a week.
const WEEK_REVIEW: &[Field] = &[
    f("week", Ty::Week, Required),
    f("done", Ty::List(&words(10), Some(6)), Empty),
    f("stalled", Ty::List(&words(10), Some(6)), Empty),
    f("time_note", SENTENCE, Text("")),
];
const PORTFOLIO: &[Field] = &[
    f(
        "greeting",
        Ty::Str {
            min: 1,
            max: 600,
            words: Some(30),
        },
        Required,
    ),
    f("focus", Ty::List(&Ty::Obj(FOCUS), Some(3)), Empty),
    f("patterns", Ty::List(&Ty::Obj(PATTERN), None), Empty),
    f("synergies", Ty::List(&Ty::Obj(SYNERGY), None), Empty),
    f("new_apps", Ty::List(&Ty::Obj(APP_IDEA), None), Empty),
    f("park_or_revive", Ty::List(&Ty::Obj(VERDICT), None), Empty),
    f("attention_note", SENTENCE, Text("")),
    f("idea_checks", Ty::List(&Ty::Obj(IDEA_CHECK), None), Empty),
    f("week_review", Ty::Nullable(&Ty::Obj(WEEK_REVIEW)), Null),
];

/// Explicit classification wins. Older feature proposals are ideas; other
/// unclassified proposals remain recommendations until an agent reviews them.
pub fn proposal_category(item: &Value, kind: &str) -> &'static str {
    match item["category"].as_str() {
        Some("idea") => "idea",
        Some("recommendation") => "recommendation",
        _ if matches!(kind, "new_app" | "owner_idea") || item["type"] == "feature" => "idea",
        _ => "recommendation",
    }
}

impl Kind {
    /// A current reading view, without obsolete agent estimates. Stored documents,
    /// unknown data and explicitly requested history remain untouched.
    pub fn current_view(self, doc: &Value) -> Value {
        let mut view = doc.clone();
        let lists: &[&str] = match self {
            Kind::Card => {
                if let Some(reentry) = view.get_mut("reentry").and_then(Value::as_object_mut) {
                    reentry.remove("effort");
                }
                &["suggestions"]
            }
            Kind::Portfolio => &["focus", "synergies", "new_apps", "idea_checks"],
        };
        for list in lists {
            for item in view
                .get_mut(*list)
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                let category = proposal_category(
                    item,
                    if *list == "new_apps" {
                        "new_app"
                    } else {
                        "suggestion"
                    },
                );
                if let Some(item) = item.as_object_mut() {
                    item.remove("effort");
                    if matches!(*list, "suggestions" | "synergies") {
                        item.insert("category".into(), json!(category));
                    }
                }
            }
        }
        view
    }

    fn fields(self) -> &'static [Field] {
        match self {
            Kind::Card => CARD,
            Kind::Portfolio => PORTFOLIO,
        }
    }

    /// Fields the schema has but a stored document lacks because it predates them, as paths such as
    /// `handbook` or `suggestions[].done_when`. Every list item contributes missing fields.
    pub fn missing_fields(self, doc: &Value) -> Vec<String> {
        fn walk(fields: &[Field], doc: &Value, path: &str, out: &mut Vec<String>) {
            for field in fields {
                let here = if path.is_empty() {
                    field.name.to_owned()
                } else {
                    format!("{path}.{}", field.name)
                };
                match doc.get(field.name) {
                    None => out.push(here),
                    Some(v) => match &field.ty {
                        Ty::Obj(inner) | Ty::Nullable(Ty::Obj(inner)) if v.is_object() => {
                            walk(inner, v, &here, out)
                        }
                        Ty::List(Ty::Obj(inner), _) => {
                            for item in v.as_array().into_iter().flatten() {
                                walk(inner, item, &format!("{here}[]"), out);
                            }
                        }
                        _ => {}
                    },
                }
            }
        }
        let mut out = Vec::new();
        walk(self.fields(), doc, "", &mut out);
        let mut seen = HashSet::new();
        out.retain(|field| seen.insert(field.clone()));
        out
    }

    /// The lists whose items carry ids: (list, prefix of generated ids, item kind).
    pub fn items(self) -> &'static [(&'static str, &'static str, &'static str)] {
        match self {
            Kind::Card => &[
                ("suggestions", "sg", "suggestion"),
                ("waiting_on_you", "q", "question"),
            ],
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
pub fn validate(
    kind: Kind,
    data: &Value,
    kept: &[String],
) -> Result<(Value, Vec<String>), Vec<String>> {
    if !data.is_object() {
        return Err(vec!["(document): the document must be a JSON object".into()]);
    }
    let mut ctx = Ctx {
        problems: Vec::new(),
        unknown: Vec::new(),
        over: Vec::new(),
        kept,
        enforce_words: true,
    };
    let clean = check_obj(kind.fields(), data, "", &mut ctx);
    if !ctx.problems.is_empty() {
        return Err(ctx.problems);
    }
    let mut clean = clean.expect("a valid object yields a value");
    // Preserve the meaning of old feature entries on partial document updates.
    for list in ["suggestions", "synergies"] {
        for (index, item) in clean
            .get_mut(list)
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
            .enumerate()
        {
            if data[list][index].get("category").is_none() {
                item["category"] = json!(proposal_category(&data[list][index], "suggestion"));
            }
        }
    }
    for category in ["idea", "recommendation"] {
        let count = clean["suggestions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["category"] == category)
            .count();
        if count > 4 {
            ctx.problems.push(format!(
                "suggestions: {count} entries in category '{category}'; limit is 4 per category"
            ));
        }
    }
    let mut assessed = HashSet::new();
    for (index, item) in clean["idea_checks"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        if let Some(idea) = item["idea"].as_i64() {
            if !assessed.insert(idea) {
                ctx.problems.push(format!(
                    "idea_checks[{index}].idea: Duplicate assessment for idea {idea}. Send one assessment per idea."
                ));
            }
        }
    }
    let mut seen = HashSet::new();
    for (list, _, _) in kind.items() {
        for (index, item) in clean[*list].as_array().into_iter().flatten().enumerate() {
            if let Some(id) = item["id"].as_str() {
                if !seen.insert(id.to_owned()) {
                    ctx.problems.push(format!("{list}[{index}].id: Duplicate id '{id}'. Keep each existing id once; omit id for a new item."));
                }
            }
        }
    }
    if !ctx.problems.is_empty() {
        return Err(ctx.problems);
    }
    for (list, prefix, _) in kind.items() {
        if let Some(Value::Array(items)) = clean.get_mut(*list) {
            for item in items {
                if item["id"].is_string() {
                    continue;
                }
                let id = loop {
                    let id = new_id(prefix);
                    if seen.insert(id.clone()) {
                        break id;
                    }
                };
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
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}.{name}")
    }
}

fn check_obj(fields: &[Field], value: &Value, path: &str, ctx: &mut Ctx) -> Option<Value> {
    let Some(map) = value.as_object() else {
        return fail(
            ctx,
            if path.is_empty() { "(document)" } else { path },
            "Use a JSON object".into(),
        );
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
        let given = map.get(field.name);
        let result = match given {
            Some(v) => check(&field.ty, v, &here, ctx),
            None => match field.default {
                Required => fail(ctx, &here, "This field is required".into()),
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
            None => fail(ctx, path, "Use a text value".into()),
            Some(s) if s.chars().count() < *min => fail(ctx, path, format!("Text has {} characters; minimum is {min}", s.chars().count())),
            Some(s) if s.chars().count() > *max => fail(ctx, path, format!("Text has {} characters; limit is {max}", s.chars().count())),
            Some(s) if words.is_some_and(|w| word_count(s) > w) => {
                let (limit, n) = (words.unwrap_or_default(), word_count(s));
                if ctx.enforce_words {
                    fail(ctx, path, format!("Text has {n} words; limit is {limit}"))
                } else {
                    ctx.over.push(format!("{path} ({n} of {limit} words)"));
                    Some(json!(s))
                }
            }
            Some(s) => Some(json!(s)),
        },
        Ty::Date => match value.as_str() {
            Some(s) if s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() => Some(json!(s)),
            _ => fail(ctx, path, "Use a valid date as YYYY-MM-DD".into()),
        },
        Ty::Week => match value.as_str() {
            Some(s) if valid_iso_week(s) => Some(json!(s)),
            _ => fail(ctx, path, "Use a valid ISO calendar week as YYYY-Www from review_week.week in portfolio context".into()),
        },
        Ty::Id => match value.as_str() {
            Some(s) if valid_item_id(s) => Some(json!(s)),
            _ => fail(ctx, path, "Use an id of 3-64 characters: lowercase letters, digits, ':', '_' or '-'; start with a letter or digit".into()),
        },
        Ty::Enum(options) => match value.as_str() {
            Some(s) if options.contains(&s) => Some(json!(s)),
            _ => {
                let quoted: Vec<String> = options.iter().map(|o| format!("'{o}'")).collect();
                fail(ctx, path, format!("Use one of {}", quoted.join(", ")))
            }
        },
        Ty::Int { min, max } => {
            let n = value.as_i64().or_else(|| {
                value.as_f64()
                    .filter(|f| f.fract() == 0.0 && *f >= i64::MIN as f64 && *f < -(i64::MIN as f64))
                    .map(|f| f as i64)
            });
            match n {
                None => fail(ctx, path, "Use a whole number".into()),
                Some(n) if n < *min || n > *max => fail(ctx, path, format!("Value {n} is outside the range {min} to {max}")),
                Some(n) => Some(json!(n)),
            }
        }
        Ty::List(item, max) => {
            let Some(items) = value.as_array() else { return fail(ctx, path, "Use a list".into()) };
            if let Some(max) = max {
                if items.len() > *max {
                    return fail(ctx, path, format!("List has {} items; limit is {max}", items.len()));
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
    s.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count()
}

fn valid_item_id(s: &str) -> bool {
    let b = s.as_bytes();
    (3..=64).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b':' | b'_' | b'-')
        })
}

fn valid_iso_week(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 8
        || &b[4..6] != b"-W"
        || !b[..4].iter().all(u8::is_ascii_digit)
        || !b[6..].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let year = s[..4].parse::<i32>().expect("four ASCII digits");
    let week = s[6..].parse::<u32>().expect("two ASCII digits");
    chrono::NaiveDate::from_isoywd_opt(year, week, chrono::Weekday::Mon).is_some()
}

fn new_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let digest = Sha1::digest(format!("{nanos}:{n}:{}", std::process::id()).as_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{prefix}-{hex}")
}

/// Replaces each list item that consists only of `{"id": ...}` with the item `lookup`
/// finds for that list and id, so agents keep an unchanged item without copying it.
/// Returns the paths of ids that `lookup` cannot resolve.
pub fn expand_kept_items(
    kind: Kind,
    payload: &mut Value,
    lookup: impl Fn(&str, &str) -> Option<Value>,
) -> Vec<String> {
    let mut missing = Vec::new();
    for (list, _, _) in kind.items() {
        let Some(items) = payload.get_mut(*list).and_then(Value::as_array_mut) else {
            continue;
        };
        for (index, item) in items.iter_mut().enumerate() {
            let Some(id) = item
                .as_object()
                .filter(|fields| fields.len() == 1)
                .and_then(|fields| fields.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
            else {
                continue;
            };
            match lookup(list, &id) {
                Some(kept) => *item = kept,
                None => missing.push(format!("{list}[{index}].id '{id}'")),
            }
        }
    }
    missing
}

/// The JSON schema of a document, generated from the same description validation uses.
pub fn json_schema(kind: Kind) -> Value {
    let mut schema = obj_schema(kind.fields());
    schema["title"] = json!(kind.title());
    let list = if kind == Kind::Card {
        "suggestions"
    } else {
        "synergies"
    };
    schema["properties"][list]["items"]["properties"]["category"]["description"] = json!("idea: new capabilities or simpler user workflows; recommendation: next steps, upkeep and repairs. Omitted feature suggestions infer idea; other omissions use recommendation.");
    if kind == Kind::Card {
        schema["properties"][list]["description"] =
            json!("At most 4 ideas and 4 recommendations; either category can be empty.");
    } else {
        schema["properties"]["idea_checks"]["description"] =
            json!("At most one assessment per idea ID.");
    }
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
        Ty::Date => {
            json!({"type": "string", "format": "date", "pattern": "^\\d{4}-\\d{2}-\\d{2}$"})
        }
        Ty::Week => {
            json!({"type": "string", "pattern": "^[0-9]{4}-W(0[1-9]|[1-4][0-9]|5[0-3])$", "description": "A valid ISO calendar week as YYYY-Www."})
        }
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
