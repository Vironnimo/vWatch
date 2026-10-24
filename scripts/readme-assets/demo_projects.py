"""Fictional projects for the README screenshots: files and back-dated commits.

Each project is a list of commits, oldest first: (days ago, message, files). A file
value of None deletes it. `WORKING` adds uncommitted changes, `BRANCHES` unmerged
branches, `WORKTREES` extra checkouts and `PUSHED` how many commits reached a
local bare `origin`.
"""

from __future__ import annotations

TIDEWATCH = [
    (61, "chore: start tidewatch", {
        "Cargo.toml": '[package]\nname = "tidewatch"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\nchrono = "0.4"\ncsv = "1"\nserde = { version = "1", features = ["derive"] }\n',
        "README.md": "# tidewatch\n\nTide tables for coastal towns, on the command line and in a small web view.\n",
        "src/main.rs": 'fn main() {\n    println!("tidewatch");\n}\n',
    }),
    (54, "feat(data): read harmonic constants per station", {
        "data/stations.csv": "id,name,lat,lon,m2_amp,m2_phase,s2_amp,s2_phase\nhusum,Husum,54.47,9.03,1.62,312,0.41,355\nbuesum,Buesum,54.12,8.86,1.58,301,0.40,344\ncuxhaven,Cuxhaven,53.87,8.72,1.43,296,0.37,338\n",
        "src/harmonics.rs": "use serde::Deserialize;\n\n#[derive(Debug, Deserialize)]\npub struct Station {\n    pub id: String,\n    pub name: String,\n    pub m2_amp: f64,\n    pub m2_phase: f64,\n    pub s2_amp: f64,\n    pub s2_phase: f64,\n}\n\npub fn load(path: &str) -> csv::Result<Vec<Station>> {\n    csv::Reader::from_path(path)?.deserialize().collect()\n}\n",
    }),
    (43, "feat(predict): water level for a station and time", {
        "src/predict.rs": "use crate::harmonics::Station;\n\nconst M2_SPEED: f64 = 28.984_104; // degrees per hour\nconst S2_SPEED: f64 = 30.0;\n\n/// Water level relative to mean sea level, in metres, `hours` after the epoch.\npub fn level(station: &Station, hours: f64) -> f64 {\n    let m2 = station.m2_amp * (M2_SPEED * hours - station.m2_phase).to_radians().cos();\n    let s2 = station.s2_amp * (S2_SPEED * hours - station.s2_phase).to_radians().cos();\n    m2 + s2\n}\n",
    }),
    (33, "feat(cli): tidewatch today <station>", {
        "src/main.rs": "mod harmonics;\nmod predict;\n\nfn main() {\n    let station = std::env::args().nth(2).unwrap_or_else(|| \"husum\".into());\n    let stations = harmonics::load(\"data/stations.csv\").expect(\"stations\");\n    let Some(station) = stations.iter().find(|s| s.id == station) else {\n        eprintln!(\"unknown station\");\n        std::process::exit(2);\n    };\n    for hour in 0..24 {\n        println!(\"{hour:02}:00  {:+.2} m\", predict::level(station, hour as f64));\n    }\n}\n",
    }),
    (26, "test(predict): compare with published Husum tables", {
        "tests/predict.rs": "#[test]\nfn husum_high_water_within_ten_centimetres() {\n    // Reference values from the published 2026 tide table.\n    assert!((1.9_f64 - 1.95).abs() < 0.1);\n}\n",
    }),
    (17, "feat(web): small axum page with today's curve", {
        "Cargo.toml": '[package]\nname = "tidewatch"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\naxum = "0.8"\nchrono = "0.4"\ncsv = "1"\nserde = { version = "1", features = ["derive"] }\ntokio = { version = "1", features = ["full"] }\n',
        "src/web.rs": "use axum::{routing::get, Router};\n\npub fn router() -> Router {\n    Router::new().route(\"/\", get(|| async { \"tidewatch\" }))\n}\n",
    }),
    (9, "feat(predict): detect high and low water", {
        "src/extremes.rs": "/// Turning points of a sampled curve: (index, level, is_high).\npub fn turning_points(levels: &[f64]) -> Vec<(usize, f64, bool)> {\n    levels\n        .windows(3)\n        .enumerate()\n        .filter_map(|(i, w)| {\n            if w[1] > w[0] && w[1] >= w[2] {\n                Some((i + 1, w[1], true))\n            } else if w[1] < w[0] && w[1] <= w[2] {\n                Some((i + 1, w[1], false))\n            } else {\n                None\n            }\n        })\n        .collect()\n}\n",
    }),
    (4, "docs: plan for station search and moon phases", {
        "docs/PLAN.md": "# Plan\n\n- [x] Prediction from M2 and S2\n- [x] High and low water\n- [ ] Station search by name\n- [ ] Moon phase overlay in the web view\n- [ ] Add more constituents (K1, O1) for the Baltic\n",
    }),
    (1, "fix(web): serve the curve as SVG", {
        "src/web.rs": "use axum::{response::Html, routing::get, Router};\n\npub fn router() -> Router {\n    Router::new().route(\"/\", get(|| async { Html(\"<svg id=\\\"curve\\\"></svg>\") }))\n}\n",
    }),
]

LEDGER = [
    (96, "chore: start ledger-lite", {
        "pyproject.toml": '[project]\nname = "ledger-lite"\nversion = "0.3.0"\nrequires-python = ">=3.12"\n',
        "README.md": "# ledger-lite\n\nHousehold budget from your bank's CSV exports. Categorizes transactions and prints a monthly report.\n",
        "ledger/__init__.py": "",
    }),
    (88, "feat(import): read bank CSV exports", {
        "ledger/importer.py": "import csv\nfrom dataclasses import dataclass\nfrom datetime import date\nfrom decimal import Decimal\n\n\n@dataclass\nclass Transaction:\n    booked: date\n    payee: str\n    amount: Decimal\n\n\ndef read(path):\n    with open(path, newline=\"\", encoding=\"utf-8\") as handle:\n        for row in csv.DictReader(handle, delimiter=\";\"):\n            day, month, year = row[\"Buchungstag\"].split(\".\")\n            amount = Decimal(row[\"Betrag\"].replace(\".\", \"\").replace(\",\", \".\"))\n            yield Transaction(date(int(year), int(month), int(day)), row[\"Empfaenger\"], amount)\n",
    }),
    (71, "feat(rules): categorize by payee patterns", {
        "ledger/rules.py": "import re\n\nRULES = [\n    (re.compile(r\"rewe|edeka|aldi\", re.I), \"Lebensmittel\"),\n    (re.compile(r\"stadtwerke|vattenfall\", re.I), \"Energie\"),\n    (re.compile(r\"miete\", re.I), \"Wohnen\"),\n]\n\n\ndef category(payee):\n    for pattern, name in RULES:\n        if pattern.search(payee):\n            return name\n    return \"Sonstiges\"  # TODO: let the user add rules without editing code\n",
    }),
    (52, "feat(report): monthly totals per category", {
        "ledger/report.py": "from collections import defaultdict\n\nfrom .rules import category\n\n\ndef monthly(transactions):\n    totals = defaultdict(lambda: defaultdict(int))\n    for t in transactions:\n        totals[t.booked.strftime(\"%Y-%m\")][category(t.payee)] += t.amount\n    return totals\n",
        "tests/test_report.py": "def test_groups_by_month():\n    assert True\n",
    }),
    (31, "fix(import): accept the new export format of the bank", {
        "ledger/importer.py": "import csv\nfrom dataclasses import dataclass\nfrom datetime import date\nfrom decimal import Decimal\n\n\n@dataclass\nclass Transaction:\n    booked: date\n    payee: str\n    amount: Decimal\n\n\nCOLUMNS = ({\"Buchungstag\", \"Empfaenger\", \"Betrag\"}, {\"Datum\", \"Zahlungsempfaenger\", \"Betrag (EUR)\"})\n\n\ndef read(path):\n    with open(path, newline=\"\", encoding=\"utf-8\") as handle:\n        rows = csv.DictReader(handle, delimiter=\";\")\n        new = \"Datum\" in rows.fieldnames\n        for row in rows:\n            day, month, year = row[\"Datum\" if new else \"Buchungstag\"].split(\".\")\n            raw = row[\"Betrag (EUR)\" if new else \"Betrag\"]\n            amount = Decimal(raw.replace(\".\", \"\").replace(\",\", \".\"))\n            payee = row[\"Zahlungsempfaenger\" if new else \"Empfaenger\"]\n            yield Transaction(date(int(year), int(month), int(day)), payee, amount)\n",
    }),
    (22, "docs: how to export from the bank", {
        "docs/export.md": "# Exporting transactions\n\nOnline banking -> Umsaetze -> Export -> CSV (semicolon).\n",
    }),
]

TRAILNOTES = [
    (40, "chore: start trailnotes", {
        "package.json": '{\n  "name": "trailnotes",\n  "version": "0.2.0",\n  "type": "module",\n  "scripts": { "dev": "vite", "test": "vitest" }\n}\n',
        "README.md": "# trailnotes\n\nPlan hikes from GPX tracks and keep notes per waypoint. Works offline in the browser.\n",
        "src/main.ts": "import { mountMap } from './map';\n\nmountMap(document.getElementById('map')!);\n",
    }),
    (34, "feat(gpx): import tracks and waypoints", {
        "src/gpx.ts": "export interface Point { lat: number; lon: number; ele?: number }\n\nexport function parseGpx(xml: string): Point[] {\n  const doc = new DOMParser().parseFromString(xml, 'application/xml');\n  return [...doc.querySelectorAll('trkpt')].map((p) => ({\n    lat: Number(p.getAttribute('lat')),\n    lon: Number(p.getAttribute('lon')),\n    ele: Number(p.querySelector('ele')?.textContent ?? NaN),\n  }));\n}\n",
    }),
    (27, "feat(map): offline tiles with a service worker", {
        "src/map.ts": "export function mountMap(element: HTMLElement) {\n  element.dataset.ready = 'true';\n}\n",
        "src/sw.ts": "self.addEventListener('fetch', () => {\n  // Cache map tiles for offline use.\n});\n",
    }),
    (19, "feat(notes): notes per waypoint in IndexedDB", {
        "src/notes.ts": "export async function saveNote(waypoint: string, text: string) {\n  const db = await openDb();\n  await db.put('notes', { waypoint, text, saved: Date.now() });\n}\n\nasync function openDb(): Promise<any> {\n  throw new Error('not implemented');\n}\n",
    }),
    (11, "feat(profile): elevation profile for a track", {
        "src/profile.ts": "import type { Point } from './gpx';\n\nexport function ascent(points: Point[]): number {\n  let total = 0;\n  for (let i = 1; i < points.length; i++) {\n    const step = (points[i].ele ?? 0) - (points[i - 1].ele ?? 0);\n    if (step > 0) total += step;\n  }\n  return total;\n}\n",
        "tests/profile.test.ts": "import { ascent } from '../src/profile';\nimport { expect, test } from 'vitest';\n\ntest('sums only climbs', () => {\n  expect(ascent([{ lat: 0, lon: 0, ele: 100 }, { lat: 0, lon: 0, ele: 150 }, { lat: 0, lon: 0, ele: 120 }])).toBe(50);\n});\n",
    }),
    (3, "fix(gpx): tracks without elevation no longer show NaN", {
        "src/gpx.ts": "export interface Point { lat: number; lon: number; ele?: number }\n\nexport function parseGpx(xml: string): Point[] {\n  const doc = new DOMParser().parseFromString(xml, 'application/xml');\n  return [...doc.querySelectorAll('trkpt')].map((p) => {\n    const ele = p.querySelector('ele')?.textContent;\n    return { lat: Number(p.getAttribute('lat')), lon: Number(p.getAttribute('lon')), ele: ele ? Number(ele) : undefined };\n  });\n}\n",
    }),
]

RECIPES = [
    (214, "chore: start recipe-box", {
        "requirements.txt": "flask==3.0.0\n",
        "README.md": "# recipe-box\n\nA small web app for family recipes with a shopping list.\n",
        "app.py": "from flask import Flask, render_template\n\napp = Flask(__name__)\n\n\n@app.route('/')\ndef index():\n    return render_template('index.html', recipes=[])\n",
    }),
    (190, "feat: recipes with ingredients and steps", {
        "models.py": "from dataclasses import dataclass, field\n\n\n@dataclass\nclass Recipe:\n    title: str\n    servings: int\n    ingredients: list[str] = field(default_factory=list)\n    steps: list[str] = field(default_factory=list)\n",
        "templates/index.html": "<h1>Rezepte</h1>\n<ul>{% for r in recipes %}<li>{{ r.title }}</li>{% endfor %}</ul>\n",
    }),
    (161, "feat: shopping list from selected recipes", {
        "shopping.py": "def shopping_list(recipes):\n    items = {}\n    for recipe in recipes:\n        for ingredient in recipe.ingredients:\n            items[ingredient] = items.get(ingredient, 0) + 1\n    return sorted(items)\n",
    }),
    (148, "wip: scale servings", {
        "models.py": "from dataclasses import dataclass, field\n\n\n@dataclass\nclass Recipe:\n    title: str\n    servings: int\n    ingredients: list[str] = field(default_factory=list)\n    steps: list[str] = field(default_factory=list)\n\n    def scaled(self, servings):\n        # TODO: ingredients are plain strings; amounts cannot be scaled yet\n        raise NotImplementedError\n",
    }),
]

PIXEL = [
    (130, "chore: start pixel-garden", {
        "index.html": "<canvas id=\"garden\" width=\"320\" height=\"180\"></canvas>\n<script type=\"module\" src=\"game.js\"></script>\n",
        "README.md": "# pixel-garden\n\nA tiny browser game: plant seeds, water them and watch the garden grow.\n",
        "game.js": "const canvas = document.getElementById('garden');\nconst ctx = canvas.getContext('2d');\nctx.fillStyle = '#7ec850';\nctx.fillRect(0, 150, 320, 30);\n",
    }),
    (112, "feat: plants grow in stages", {
        "plants.js": "export const STAGES = ['seed', 'sprout', 'bud', 'flower'];\n\nexport function grow(plant, watered) {\n  if (watered && plant.stage < STAGES.length - 1) plant.stage += 1;\n  return plant;\n}\n",
    }),
    (97, "feat: save the garden in localStorage", {
        "save.js": "export function save(garden) {\n  localStorage.setItem('garden', JSON.stringify(garden));\n}\n\nexport function load() {\n  return JSON.parse(localStorage.getItem('garden') ?? '[]');\n}\n",
    }),
    (90, "release: 1.0", {
        "CHANGELOG.md": "# 1.0\n\nPlant, water, grow and save.\n",
    }),
    (45, "fix: plants no longer vanish after reload in Safari", {
        "save.js": "export function save(garden) {\n  try {\n    localStorage.setItem('garden', JSON.stringify(garden));\n  } catch {\n    // Private mode: keep the garden in memory only.\n  }\n}\n\nexport function load() {\n  try {\n    return JSON.parse(localStorage.getItem('garden') ?? '[]');\n  } catch {\n    return [];\n  }\n}\n",
    }),
]

PROJECTS = {
    "tidewatch": TIDEWATCH,
    "ledger-lite": LEDGER,
    "trailnotes": TRAILNOTES,
    "recipe-box": RECIPES,
    "pixel-garden": PIXEL,
}

PUSHED = {"tidewatch": 7, "trailnotes": 6, "pixel-garden": 5}

TAGS = {"pixel-garden": ("v1.0", 3)}

WORKING = {
    "tidewatch": {
        "src/moon.rs": "/// Moon phase for a date, 0.0 = new moon, 0.5 = full moon.\npub fn phase(days_since_new_moon: f64) -> f64 {\n    (days_since_new_moon / 29.530_588).fract()\n}\n",
    },
    "ledger-lite": {
        "ledger/rules.py": "import re\n\nRULES = [\n    (re.compile(r\"rewe|edeka|aldi|lidl\", re.I), \"Lebensmittel\"),\n    (re.compile(r\"stadtwerke|vattenfall\", re.I), \"Energie\"),\n    (re.compile(r\"miete\", re.I), \"Wohnen\"),\n]\n\n\ndef category(payee):\n    for pattern, name in RULES:\n        if pattern.search(payee):\n            return name\n    return \"Sonstiges\"  # TODO: let the user add rules without editing code\n",
    },
}

BRANCHES = {
    "ledger-lite": ("feature/pdf-export", 15, "feat(report): export the monthly report as PDF", {
        "ledger/pdf.py": "def export(report, path):\n    raise NotImplementedError('pick a PDF library first')\n",
    }),
}

WORKTREES = {
    "trailnotes": ("trailnotes-offline-sync", "fix/offline-sync"),
}
