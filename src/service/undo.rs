use super::*;
use crate::store::OwnerUndo;

fn describe(value: &Value) -> String {
    match value {
        Value::Null => "absent".into(),
        Value::Bool(value) => if *value { "yes" } else { "no" }.into(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(values) => values.iter().map(describe).collect::<Vec<_>>().join(", "),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| format!("{key}: {}", describe(value)))
            .collect::<Vec<_>>()
            .join("; "),
    }
}

/// Attach the stored outcome to the established response shape.
pub(super) fn with_receipt(mut result: Value, receipt: Value) -> Value {
    if let (Some(result), Some(receipt)) = (result.as_object_mut(), receipt.as_object()) {
        result.extend(
            receipt
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
    result
}

impl Watchtower {
    /// Restore exactly the fields changed by one recorded owner edit.
    pub fn undo(&self, token: &str) -> Result<Value> {
        match self.store.undo_owner_change(token,&self.now())? {
            OwnerUndo::Restored(mut result) => {
                if result["changed"] == true && result["target"]["fields"].as_array().is_some_and(|fields| fields.iter().any(|field| field == "path")) {
                    result["notes"] = json!(["The folder registration was restored. Facts from the other folder were discarded; measure the restored folder before using its facts."]);
                    result["next"] = json!(format!("watchtower facts refresh {}",result["target"]["project"].as_str().unwrap_or("")));
                }
                Ok(result)
            },
            OwnerUndo::Missing => Err(WatchtowerError::new("unknown_undo",format!("No change has undo token '{token}'. Nothing was changed."))
                .hint("Use the exact undo command returned by the change in this Watchtower instance.")),
            OwnerUndo::Conflict {target,current} => {
                Err(WatchtowerError::new("undo_conflict",format!("The affected values changed after this operation ({}) or its undo. Nothing was changed. Current values: {}.",describe(&target),describe(&current)))
                    .hint("Keep these newer values, or use the undo command from their most recent change. This older undo cannot overwrite them."))
            }
            OwnerUndo::PathTaken(id) => Err(WatchtowerError::new("undo_conflict",format!("The previous folder belongs to project '{id}'. Nothing was changed."))
                .hint(format!("Keep the current folder, or resolve that project's registration before repeating this undo: watchtower projects show {id}"))),
        }
    }

    /// Validate the complete preference edit before saving all keys together.
    pub fn update_prefs(&self, updates: &[(&str, Value)]) -> Result<Value> {
        for (key, value) in updates {
            self.validate_pref(key, value)?;
        }
        let receipt = self.store.set_owner_prefs(updates, &self.now())?;
        let mut prefs = Map::new();
        for key in PREFS {
            prefs.insert((*key).into(), self.store.pref(key)?.unwrap_or(Value::Null));
        }
        Ok(with_receipt(json!({"prefs":prefs}), receipt))
    }
}
