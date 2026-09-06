use serde_json::Value;
use vde_worktree::cli::contract::{data_schema, envelope_schema};

pub fn parse_cli_json(stdout: &[u8]) -> Value {
    let value: Value = serde_json::from_slice(stdout).expect("stdout is one JSON object");
    validate(&value, &envelope_schema(), "$").unwrap_or_else(|error| panic!("{error}\n{value}"));
    let status = value["status"].as_str().expect("status");
    assert_eq!(value["error"].is_null(), status == "ok", "{value}");
    if value["data"].is_null() {
        assert_eq!(status, "error", "successful output requires command data");
    } else {
        let command = value["command"].as_str().expect("command");
        validate(&value["data"], &data_schema(command), "$.data")
            .unwrap_or_else(|error| panic!("{command}: {error}\n{value}"));
    }
    value
}

// Validate the JSON Schema vocabulary used by describe, against independently produced runtime
// results. Unknown schema types are rejected so extending the vocabulary cannot silently weaken it.
fn validate(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if let Some(expected) = schema.get("const")
        && value != expected
    {
        return Err(format!("{path}: expected constant {expected}, got {value}"));
    }
    if let Some(options) = schema.get("enum")
        && !options.as_array().expect("enum array").contains(value)
    {
        return Err(format!("{path}: {value} is not in {options}"));
    }
    if let Some(options) = schema.get("anyOf") {
        let failures = options
            .as_array()
            .expect("anyOf array")
            .iter()
            .filter_map(|schema| validate(value, schema, path).err())
            .collect::<Vec<_>>();
        if failures.len() == options.as_array().unwrap().len() {
            return Err(failures.join("; "));
        }
    }
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        let valid = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "boolean" => value.is_boolean(),
            "integer" => value.is_i64() || value.is_u64(),
            "null" => value.is_null(),
            unsupported => panic!("unvalidated schema type: {unsupported}"),
        };
        if !valid {
            return Err(format!("{path}: expected {kind}, got {value}"));
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required") {
            for key in required.as_array().expect("required array") {
                let key = key.as_str().expect("required key");
                if !object.contains_key(key) {
                    return Err(format!("{path}: missing {key}"));
                }
            }
        }
        for (key, value) in object {
            if let Some(child) = schema
                .get("properties")
                .and_then(|properties| properties.get(key))
            {
                validate(value, child, &format!("{path}.{key}"))?;
            } else if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                return Err(format!("{path}: undeclared field {key}"));
            }
        }
    }
    if let Some(items) = schema.get("items")
        && let Some(array) = value.as_array()
    {
        for (index, value) in array.iter().enumerate() {
            validate(value, items, &format!("{path}[{index}]"))?;
        }
    }
    Ok(())
}
