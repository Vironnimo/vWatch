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
pub(super) fn with_receipt(mut result: Value, receipt: crate::store::OwnerReceipt) -> Value {
    let fields = result.as_object_mut().expect("service response object");
    fields.extend(Map::from_iter([
        ("changed".into(), json!(receipt.changed)),
        ("target".into(), receipt.target),
        ("previous".into(), receipt.previous),
        ("current".into(), receipt.current),
    ]));
    if let Some(token) = receipt.token {
        fields.insert("undo".into(), json!(format!("vwatch undo {token}")));
        fields.insert("undo_token".into(), json!(token));
    }
    result
}

impl VWatch {
    /// Restore exactly the fields changed by one recorded owner edit.
    pub fn undo(&self, token: &str) -> Result<Value> {
        match self.store.undo_owner_change(token,&self.now())? {
            OwnerUndo::Restored(mut result) => {
                if result["changed"] == true && result["target"]["fields"].as_array().is_some_and(|fields| fields.iter().any(|field| field == "path")) {
                    result["notes"] = json!(["The folder registration was restored. Facts from the other folder were discarded; measure the restored folder before using its facts."]);
                    result["next"] = json!(format!("vwatch facts refresh {}",result["target"]["project"].as_str().unwrap_or("")));
                }
                Ok(result)
            },
            OwnerUndo::Missing => Err(VWatchError::new("unknown_undo",format!("No change has undo token '{token}'. Nothing was changed."))
                .hint("Use the exact undo command returned by the change in this vWatch instance.")),
            OwnerUndo::Conflict {target,current} => {
                Err(VWatchError::new("undo_conflict",format!("The affected values changed after this operation ({}) or its undo. Nothing was changed. Current values: {}.",describe(&target),describe(&current)))
                    .hint("Keep these newer values, or use the undo command from their most recent change. This older undo cannot overwrite them."))
            }
            OwnerUndo::PathTaken(id) => Err(VWatchError::new("undo_conflict",format!("The previous folder belongs to project '{id}'. Nothing was changed."))
                .hint(format!("Keep the current folder, or resolve that project's registration before repeating this undo: vwatch projects show {id}"))),
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
