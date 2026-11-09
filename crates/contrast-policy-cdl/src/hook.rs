//! フック `windowOpen` の呼び出しと、戻り値の検査（`CdlPolicy`）。
//!
//! CDL のワークスペース番号は 1 始まり、`contrast-policy` は 0 始まり。範囲外・非整数・非有限の値は丸めずに誤りにする。
//! 誤りは診断で返し、呼び出し側（`Config::place`）がその呼び出しだけ既定の配置に戻す。

use std::collections::BTreeMap;
use std::rc::Rc;

use cdl_host::{Program, Value};
use contrast_policy::{
    ConfigDiagnostic, DiagnosticSeverity, HookPlacement, PlacementHook, WindowInfo, codes,
};

use crate::diag::line_column;

/// 読み込んだ `Program` の `windowOpen` を呼ぶフック。
pub struct CdlPolicy {
    /// 評価済みのプログラム（不変）。
    program: Rc<Program>,
    /// 診断の位置を求めるためのソース本文（id → 本文）。
    sources: BTreeMap<String, String>,
}

impl CdlPolicy {
    /// プログラムとソース本文から作る。
    pub(crate) fn new(program: Rc<Program>, sources: BTreeMap<String, String>) -> Self {
        Self { program, sources }
    }
}

/// 位置のない実行時の診断（警告）。
fn runtime(code: &str, message: impl Into<String>) -> ConfigDiagnostic {
    ConfigDiagnostic {
        severity: DiagnosticSeverity::Warning,
        code: code.to_owned(),
        file: String::new(),
        line: None,
        column: None,
        message: message.into(),
    }
}

/// 0 始まりの `u8` を CDL の 1 始まりの数にする。
fn to_cdl_workspace(index: u8) -> f64 {
    f64::from(index) + 1.0
}

/// フックに渡す引数の値。
pub(crate) fn window_info_value(w: &WindowInfo) -> Value {
    let mut fields = vec![
        ("appId", Value::string(&w.app_id)),
        ("isDialog", Value::Bool(w.is_dialog)),
        ("workspace", Value::Number(to_cdl_workspace(w.workspace))),
    ];
    if let Some(title) = &w.title {
        fields.push(("title", Value::string(title)));
    }
    Value::object(fields)
}

/// 戻り値のワークスペース番号（1 始まり）を 0 始まりにする。
fn workspace_from_value(n: f64) -> Result<u8, ConfigDiagnostic> {
    if !n.is_finite() || n.fract() != 0.0 || n < 1.0 {
        return Err(runtime(
            codes::HOOK_BAD_WORKSPACE,
            format!("windowOpen の workspace は 1 以上の整数（{n} は使えない）"),
        ));
    }
    if n > 255.0 {
        return Err(runtime(
            codes::HOOK_WORKSPACE_RANGE,
            format!("windowOpen の workspace {n} はワークスペース数を超える"),
        ));
    }
    u8::try_from(n as u32)
        .ok()
        .and_then(|v| v.checked_sub(1))
        .ok_or_else(|| runtime(codes::HOOK_BAD_WORKSPACE, "windowOpen の workspace が不正"))
}

/// フックの戻り値（`Placement`）を `HookPlacement` にする。
pub(crate) fn placement_from_value(v: &Value) -> Result<HookPlacement, ConfigDiagnostic> {
    let shape = |what: &str| {
        runtime(
            codes::HOOK_BAD_SHAPE,
            format!("windowOpen の戻り値: {what}"),
        )
    };
    let Value::Object(fields) = v else {
        return Err(shape("オブジェクトではない"));
    };
    let floating = match fields.get("floating") {
        Some(Value::Bool(b)) => *b,
        _ => return Err(shape("`floating` は boolean が要る")),
    };
    let workspace = match fields.get("workspace") {
        None | Some(Value::Undefined) => None,
        Some(Value::Number(n)) => Some(workspace_from_value(*n)?),
        Some(_) => return Err(shape("`workspace` は number が要る")),
    };
    let focus = match fields.get("focus") {
        None | Some(Value::Undefined) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(shape("`focus` は boolean が要る")),
    };
    let fullscreen = match fields.get("fullscreen") {
        None | Some(Value::Undefined) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(shape("`fullscreen` は boolean が要る")),
    };
    // niri の窓ルールの `default-column-width {}`（窓に決めさせる）は "window"。
    let column_width = match fields.get("columnWidth") {
        None | Some(Value::Undefined) => None,
        Some(Value::String(s)) if &**s == "window" => Some(contrast_policy::PlacementWidth::Window),
        Some(_) => {
            return Err(shape(
                "`columnWidth` は \"window\" だけ（niri の default-column-width {}）",
            ));
        }
    };
    Ok(HookPlacement {
        workspace,
        floating,
        focus,
        fullscreen,
        column_width,
    })
}

impl PlacementHook for CdlPolicy {
    fn window_open(&self, window: &WindowInfo) -> Result<Option<HookPlacement>, ConfigDiagnostic> {
        match self
            .program
            .call_hook("windowOpen", window_info_value(window))
        {
            Ok(None) => Ok(None),
            Ok(Some(v)) => placement_from_value(&v).map(Some),
            Err(d) => {
                let (line, column) = self
                    .sources
                    .get(&d.span.file)
                    .and_then(|text| line_column(text, d.span.start))
                    .unzip();
                Err(ConfigDiagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: codes::HOOK_FAILED.to_owned(),
                    file: d.span.file.clone(),
                    line,
                    column,
                    message: format!("windowOpen: {}[{}]", d.message, d.code.as_str()),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 戻り値の `Value`。
    fn placement(ws: Option<Value>, floating: Value) -> Value {
        let mut f = vec![("floating", floating)];
        if let Some(w) = ws {
            f.push(("workspace", w));
        }
        Value::object(f)
    }

    /// 診断コード（失敗でなければ空）。
    fn code(r: Result<HookPlacement, ConfigDiagnostic>) -> String {
        r.err().map(|d| d.code).unwrap_or_default()
    }

    #[test]
    fn workspace_is_converted_to_zero_based() {
        let p = placement_from_value(&placement(Some(Value::Number(3.0)), Value::Bool(true)));
        assert_eq!(
            p,
            Ok(HookPlacement {
                workspace: Some(2),
                floating: true,
                focus: true,
                fullscreen: false,
                column_width: None,
            })
        );
    }

    #[test]
    fn omitted_workspace_and_focus_take_defaults() {
        assert_eq!(
            placement_from_value(&placement(None, Value::Bool(false))),
            Ok(HookPlacement {
                workspace: None,
                floating: false,
                focus: true,
                fullscreen: false,
                column_width: None,
            })
        );
    }

    #[test]
    fn fullscreen_is_read_and_defaults_to_false() {
        let with = |v: Option<Value>| {
            let mut fields = vec![("floating", Value::Bool(false))];
            if let Some(v) = v {
                fields.push(("fullscreen", v));
            }
            placement_from_value(&Value::object(fields))
        };
        assert_eq!(with(None).map(|p| p.fullscreen), Ok(false));
        assert_eq!(
            with(Some(Value::Undefined)).map(|p| p.fullscreen),
            Ok(false)
        );
        assert_eq!(
            with(Some(Value::Bool(true))).map(|p| p.fullscreen),
            Ok(true)
        );
        assert_eq!(
            with(Some(Value::Bool(false))).map(|p| p.fullscreen),
            Ok(false)
        );
        assert_eq!(code(with(Some(Value::Number(1.0)))), codes::HOOK_BAD_SHAPE);
    }

    #[test]
    fn invalid_workspace_values_are_rejected() {
        for n in [0.0, -1.0, 1.5, f64::NAN, f64::INFINITY, -0.5] {
            let r = placement_from_value(&placement(Some(Value::Number(n)), Value::Bool(false)));
            assert_eq!(code(r), codes::HOOK_BAD_WORKSPACE, "{n}");
        }
        let r = placement_from_value(&placement(Some(Value::Number(300.0)), Value::Bool(false)));
        assert_eq!(code(r), codes::HOOK_WORKSPACE_RANGE);
    }

    #[test]
    fn bad_shapes_are_rejected() {
        assert_eq!(
            code(placement_from_value(&Value::Number(1.0))),
            codes::HOOK_BAD_SHAPE
        );
        assert_eq!(
            code(placement_from_value(&Value::object([(
                "workspace",
                Value::Number(1.0)
            )]))),
            codes::HOOK_BAD_SHAPE
        );
        assert_eq!(
            code(placement_from_value(&placement(
                Some(Value::string("2")),
                Value::Bool(false)
            ))),
            codes::HOOK_BAD_SHAPE
        );
    }

    #[test]
    fn window_info_is_one_based_and_omits_missing_title() {
        let v = window_info_value(&WindowInfo {
            app_id: String::new(),
            title: None,
            is_dialog: true,
            workspace: 0,
        });
        let Value::Object(f) = v else {
            return;
        };
        assert_eq!(f.get("workspace"), Some(&Value::Number(1.0)));
        assert_eq!(f.get("appId"), Some(&Value::string("")));
        assert!(!f.contains_key("title"));
    }
}
