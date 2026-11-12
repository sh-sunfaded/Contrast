//! CDL の診断 (`cdl_host::Diagnostic`) を CDL 非依存の `ConfigDiagnostic` に写す。
//!
//! CDL の位置はバイトオフセットなので、読んだソースの本文から行・列（1 始まり、列は文字単位）を求める。
//! 位置を求められない（ソースが無い・範囲外・文字境界でない）ときは `line` / `column` を `None` にする。

use std::collections::BTreeMap;

use cdl_host::Diagnostic;

use contrast_policy::{ConfigDiagnostic, DiagnosticSeverity};

/// ファイル名（空）で位置を持たない、実装側の不具合などの診断を作る。
pub(crate) fn internal(code: &str, message: impl Into<String>) -> ConfigDiagnostic {
    ConfigDiagnostic {
        severity: DiagnosticSeverity::Error,
        code: code.to_owned(),
        file: String::new(),
        line: None,
        column: None,
        message: message.into(),
    }
}

/// 要素の位置 `span` に Contrast の診断（CT…）を作る。位置を求められなければ行・列は `None`。
pub(crate) fn at(
    span: &cdl_host::diag::Span,
    code: &str,
    message: impl Into<String>,
    sources: &BTreeMap<String, String>,
) -> ConfigDiagnostic {
    let (line, column) = sources
        .get(&span.file)
        .and_then(|text| line_column(text, span.start))
        .unzip();
    ConfigDiagnostic {
        severity: DiagnosticSeverity::Error,
        code: code.to_owned(),
        file: span.file.clone(),
        line,
        column,
        message: message.into(),
    }
}

/// 1 件の診断を写す。`sources` は id → 本文。
pub(crate) fn convert(d: &Diagnostic, sources: &BTreeMap<String, String>) -> ConfigDiagnostic {
    let severity = match d.severity.as_str() {
        "warning" => DiagnosticSeverity::Warning,
        "hint" => DiagnosticSeverity::Hint,
        _ => DiagnosticSeverity::Error,
    };
    let (line, column) = sources
        .get(&d.span.file)
        .and_then(|text| line_column(text, d.span.start))
        .unzip();
    ConfigDiagnostic {
        severity,
        code: d.code.as_str().to_owned(),
        file: d.span.file.clone(),
        line,
        column,
        message: d.message.clone(),
    }
}

/// 全件を写す。
pub(crate) fn convert_all(
    ds: &[Diagnostic],
    sources: &BTreeMap<String, String>,
) -> Vec<ConfigDiagnostic> {
    ds.iter().map(|d| convert(d, sources)).collect()
}

/// バイトオフセット `start` の行と列（どちらも 1 始まり）。範囲外や文字境界でなければ `None`。
pub(crate) fn line_column(text: &str, start: u32) -> Option<(u32, u32)> {
    let start = usize::try_from(start).ok()?;
    let before = text.get(..start)?;
    let line = before.matches('\n').count().checked_add(1)?;
    let line_start = before.rfind('\n').map_or(0, |i| i.saturating_add(1));
    let column = before.get(line_start..)?.chars().count().checked_add(1)?;
    Some((u32::try_from(line).ok()?, u32::try_from(column).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_byte_is_line_one_column_one() {
        assert_eq!(line_column("abc", 0), Some((1, 1)));
    }

    #[test]
    fn counts_lines_and_chars_not_bytes() {
        // 「あ」は 3 バイト。2 行目の「い」の直後（バイト 3 + 1 + 3 = 7）は 2 行目 2 列。
        let text = "x\nあい\n";
        assert_eq!(line_column(text, 2), Some((2, 1)));
        assert_eq!(line_column(text, 5), Some((2, 2)));
    }

    #[test]
    fn out_of_range_or_mid_char_is_none() {
        assert_eq!(line_column("abc", 4), None);
        // 「あ」の途中のバイト。
        assert_eq!(line_column("あ", 1), None);
    }
}
