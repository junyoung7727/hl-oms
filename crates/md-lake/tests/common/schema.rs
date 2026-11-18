//! A test-only validator for the JSON-schema subset the contract schemas use
//! (type, const, enum, pattern, properties, required, additionalProperties,
//! items, oneOf, minimum, maximum, minItems, minLength).

use serde_json::Value;

fn type_ok(t: &str, v: &Value) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "integer" => v.is_i64() || v.is_u64(),
        "number" => v.is_number(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        _ => false,
    }
}

pub fn errors(schema: &Value, v: &Value, at: &str, out: &mut Vec<String>) {
    if let Some(t) = schema.get("type") {
        let ok = match t {
            Value::String(t) => type_ok(t, v),
            Value::Array(ts) => ts.iter().any(|t| t.as_str().is_some_and(|t| type_ok(t, v))),
            _ => true,
        };
        if !ok {
            out.push(format!("{at}: type {t} for {v}"));
            return;
        }
    }
    if let Some(c) = schema.get("const") {
        if c != v {
            out.push(format!("{at}: const {c} for {v}"));
        }
    }
    if let Some(Value::Array(e)) = schema.get("enum") {
        if !e.contains(v) {
            out.push(format!("{at}: not in enum: {v}"));
        }
    }
    if let (Some(p), Some(s)) = (schema.get("pattern").and_then(Value::as_str), v.as_str()) {
        if !regex_lite::Regex::new(p).expect("pattern").is_match(s) {
            out.push(format!("{at}: {s:?} does not match {p}"));
        }
    }
    if let (Some(m), Some(s)) = (schema.get("minLength").and_then(Value::as_u64), v.as_str()) {
        if (s.chars().count() as u64) < m {
            out.push(format!("{at}: shorter than {m}"));
        }
    }
    if let Some(n) = v.as_f64() {
        if schema
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|m| n < m)
        {
            out.push(format!("{at}: below minimum"));
        }
        if schema
            .get("maximum")
            .and_then(Value::as_f64)
            .is_some_and(|m| n > m)
        {
            out.push(format!("{at}: above maximum"));
        }
    }
    if let Some(obj) = v.as_object() {
        let props = schema.get("properties").and_then(Value::as_object);
        if let Some(Value::Array(req)) = schema.get("required") {
            for r in req.iter().filter_map(Value::as_str) {
                if !obj.contains_key(r) {
                    out.push(format!("{at}: missing {r}"));
                }
            }
        }
        for (k, x) in obj {
            match props.and_then(|p| p.get(k)) {
                Some(s) => errors(s, x, &format!("{at}.{k}"), out),
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    out.push(format!("{at}: unknown key {k}"))
                }
                None => {}
            }
        }
    }
    if let Some(arr) = v.as_array() {
        if let Some(m) = schema.get("minItems").and_then(Value::as_u64) {
            if (arr.len() as u64) < m {
                out.push(format!("{at}: fewer than {m} items"));
            }
        }
        if let Some(items) = schema.get("items") {
            for (i, x) in arr.iter().enumerate() {
                errors(items, x, &format!("{at}[{i}]"), out);
            }
        }
    }
    if let Some(Value::Array(one)) = schema.get("oneOf") {
        let passing = one
            .iter()
            .filter(|s| {
                let mut e = Vec::new();
                errors(s, v, at, &mut e);
                e.is_empty()
            })
            .count();
        if passing != 1 {
            out.push(format!("{at}: {passing} of the oneOf branches match"));
        }
    }
}

pub fn assert_valid(schema_rel: &str, v: &Value) {
    let path = super::golden_dir().join("..").join(schema_rel);
    let schema: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("schema")).expect("schema json");
    let mut out = Vec::new();
    errors(&schema, v, "$", &mut out);
    assert!(out.is_empty(), "{schema_rel}: {out:#?}\n{v}");
}
