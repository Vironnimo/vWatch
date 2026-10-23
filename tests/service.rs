mod common;
use chrono::Duration;
use common::{card, card_with, demo_repo, git, service};
use serde_json::{json, Value};

#[path = "service/documents.rs"]
mod documents;
#[path = "service/feedback.rs"]
mod feedback;
#[path = "service/freshness.rs"]
mod freshness;
#[path = "service/projects.rs"]
mod projects;
#[path = "service/reading.rs"]
mod reading;
