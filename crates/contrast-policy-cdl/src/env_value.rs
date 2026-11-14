//! `PolicyEnv`（CDL 非依存）を、CDL の `Env` の値（`Value::Object`）にする。型は `schema::env_type` と同じ形。
//! 省略できるフィールド（`workspace.name`・`focus`）は、無いときは渡さない（`undefined` を渡すのと同じ。`impl_env_probe.rs`）。

use cdl_host::Value;
use contrast_policy::PolicyEnv;

/// `env` を CDL の値にする。
pub(crate) fn env_value(env: &PolicyEnv) -> Value {
    let mut workspace = vec![
        ("index", Value::Number(f64::from(env.workspace.index))),
        (
            "windowCount",
            Value::Number(f64::from(env.workspace.window_count)),
        ),
    ];
    if let Some(name) = &env.workspace.name {
        workspace.push(("name", Value::string(name)));
    }
    let mut fields = vec![
        ("profile", Value::string(&env.profile)),
        ("mode", Value::string(&env.mode)),
        (
            "output",
            Value::object([("name", Value::string(&env.output_name))]),
        ),
        ("workspace", Value::object(workspace)),
    ];
    if let Some(focus) = &env.focus {
        fields.push((
            "focus",
            Value::object([
                ("appId", Value::string(&focus.app_id)),
                ("isDialog", Value::Bool(focus.is_dialog)),
                ("state", Value::string(focus.state.as_str())),
            ]),
        ));
    }
    Value::object(fields)
}

#[cfg(test)]
mod tests {
    use contrast_policy::{FocusEnv, FocusState};

    use super::*;

    #[test]
    fn omitted_fields_are_not_passed() {
        let value = env_value(&PolicyEnv::baseline("p"));
        let Value::Object(map) = value else {
            return;
        };
        assert!(map.contains_key("profile") && map.contains_key("workspace"));
        assert!(!map.contains_key("focus"));
        let mut env = PolicyEnv::baseline("p");
        env.focus = Some(FocusEnv {
            app_id: "foot".to_owned(),
            is_dialog: false,
            state: FocusState::Floating,
        });
        let Value::Object(map) = env_value(&env) else {
            return;
        };
        assert!(map.contains_key("focus"));
    }
}
