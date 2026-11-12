//! EDID（モニタが持つ識別情報）の読み取りと、出力の識別（make・model・serial）。
//!
//! niri は libdisplay-info で make・model・serial を読み、出力の照合（設定の `output "…"`）と自動配置の順に使う
//! （`src/utils/mod.rs` の `OutputName`、`src/niri.rs` の出力の並び）。Contrast は依存を足さず、EDID の基本ブロックを自前で読む。
//! 製造元は、EDID の 3 文字の PNP ID（例: `DEL`）のまま使う（niri は PNP の表から会社名にする。ここが違う）。

use std::cmp::Ordering;

/// EDID から読んだ識別情報。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Edid {
    /// 製造元の PNP ID（3 文字）。
    pub make: String,
    /// モデル名（モニタ名の記述子。無ければ製品コードの 16 進）。
    pub model: String,
    /// シリアル（シリアルの記述子。無ければ、0 でなければ数値のシリアル）。
    pub serial: Option<String>,
}

/// 記述子（18 バイト）の文字列（13 バイト、改行か 0 で終わる）。
fn descriptor_text(block: &[u8]) -> Option<String> {
    let text = block.get(5..18)?;
    let end = text
        .iter()
        .position(|b| *b == 0x0A || *b == 0)
        .unwrap_or(text.len());
    let s: String = text
        .get(..end)?
        .iter()
        .filter(|b| b.is_ascii_graphic() || **b == b' ')
        .map(|b| char::from(*b))
        .collect();
    let s = s.trim().to_owned();
    (!s.is_empty()).then_some(s)
}

/// EDID の基本ブロック（128 バイト以上）を読む。ヘッダが違えば `None`。
pub fn parse_edid(data: &[u8]) -> Option<Edid> {
    if data.get(..8)? != [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00] {
        return None;
    }
    let id = u16::from_be_bytes([*data.get(8)?, *data.get(9)?]);
    let letter = |shift: u32| char::from(b'A' + (((id >> shift) & 0x1F) as u8).saturating_sub(1));
    let make: String = [letter(10), letter(5), letter(0)].iter().collect();
    let product = u16::from_le_bytes([*data.get(10)?, *data.get(11)?]);
    let numeric = u32::from_le_bytes([
        *data.get(12)?,
        *data.get(13)?,
        *data.get(14)?,
        *data.get(15)?,
    ]);
    let mut name = None;
    let mut serial = None;
    for start in [54, 72, 90, 108] {
        let Some(block) = data.get(start..start + 18) else {
            continue;
        };
        // 表示用の記述子は、先頭の 3 バイトが 0。5 番目の 4 バイト目が種類。
        if block.get(..3) != Some(&[0, 0, 0][..]) {
            continue;
        }
        match block.get(3) {
            Some(0xFC) => name = descriptor_text(block),
            Some(0xFF) => serial = descriptor_text(block),
            _ => {}
        }
    }
    Some(Edid {
        make,
        model: name.unwrap_or_else(|| format!("{product:04x}")),
        serial: serial.or_else(|| (numeric != 0).then(|| numeric.to_string())),
    })
}

/// 出力の識別（`smithay::output::Output` の user data に置く）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputIdentity {
    /// 接続の名前（`eDP-1`）。
    pub connector: String,
    /// 製造元（分からなければ `None`）。
    pub make: Option<String>,
    /// モデル（分からなければ `None`）。
    pub model: Option<String>,
    /// シリアル（分からなければ `None`）。
    pub serial: Option<String>,
}

impl OutputIdentity {
    /// 接続の名前だけの識別（EDID が読めない・winit）。
    pub fn connector_only(connector: &str) -> Self {
        Self {
            connector: connector.to_owned(),
            make: None,
            model: None,
            serial: None,
        }
    }

    /// EDID から作る。
    pub fn from_edid(connector: &str, edid: Option<&Edid>) -> Self {
        Self {
            connector: connector.to_owned(),
            make: edid.map(|e| e.make.clone()),
            model: edid.map(|e| e.model.clone()),
            serial: edid.and_then(|e| e.serial.clone()),
        }
    }

    /// core に渡す出力の名前。シリアルが分かれば `make model serial`（別の接続に挿し替えても同じモニタと分かる）、
    /// 分からなければ接続の名前（同じ機種が 2 台あっても名前が衝突しない）。niri の `format_make_model_serial_or_connector` に近い。
    pub fn core_name(&self) -> String {
        match (&self.make, &self.model, &self.serial) {
            (Some(make), Some(model), Some(serial)) => format!("{make} {model} {serial}"),
            _ => self.connector.clone(),
        }
    }

    /// 自動配置の並びの比較（niri の `OutputName::compare`）: make・model・serial を先に、同じか不明なら接続の名前。
    pub fn compare(&self, other: &Self) -> Ordering {
        let key = |i: &Self| {
            (
                i.make.clone().unwrap_or_default(),
                i.model.clone().unwrap_or_default(),
                i.serial.clone().unwrap_or_default(),
            )
        };
        key(self)
            .cmp(&key(other))
            .then_with(|| self.connector.cmp(&other.connector))
    }
}

/// 出力の識別（DRM が user data に置いたもの）。無ければ（winit）接続の名前だけ。
pub fn identity_of(output: &smithay::output::Output) -> OutputIdentity {
    output
        .user_data()
        .get::<OutputIdentity>()
        .cloned()
        .unwrap_or_else(|| OutputIdentity::connector_only(&output.name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 試験用の EDID: 製造元 DEL、製品コード 0x4141、数値のシリアル 0x01020304、モニタ名 "DELL U2720Q"、シリアルの記述子 "ABC123"。
    fn sample() -> Vec<u8> {
        let mut d = vec![0u8; 128];
        d[..8].copy_from_slice(&[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
        // DEL: D=4, E=5, L=12 → 00100 00101 01100
        let id: u16 = (4 << 10) | (5 << 5) | 12;
        d[8..10].copy_from_slice(&id.to_be_bytes());
        d[10..12].copy_from_slice(&0x4141_u16.to_le_bytes());
        d[12..16].copy_from_slice(&0x0102_0304_u32.to_le_bytes());
        let mut put = |start: usize, tag: u8, text: &str| {
            d[start + 3] = tag;
            let mut t = text.as_bytes().to_vec();
            t.push(0x0A);
            t.resize(13, b' ');
            d[start + 5..start + 18].copy_from_slice(&t);
        };
        put(54, 0xFC, "DELL U2720Q");
        put(72, 0xFF, "ABC123");
        d
    }

    #[test]
    fn make_model_and_serial_come_from_the_descriptors() {
        let e = parse_edid(&sample()).expect("edid");
        assert_eq!(e.make, "DEL");
        assert_eq!(e.model, "DELL U2720Q");
        assert_eq!(e.serial.as_deref(), Some("ABC123"));
    }

    #[test]
    fn without_descriptors_the_product_code_and_numeric_serial_are_used() {
        let mut d = sample();
        for start in [54, 72] {
            d[start + 3] = 0x10;
        }
        let e = parse_edid(&d).expect("edid");
        assert_eq!(e.model, "4141");
        assert_eq!(e.serial.as_deref(), Some("16909060"));
        d[12..16].copy_from_slice(&[0; 4]);
        assert_eq!(parse_edid(&d).expect("edid").serial, None);
    }

    #[test]
    fn a_bad_header_or_a_short_blob_is_refused() {
        assert_eq!(parse_edid(&[0; 128]), None);
        assert_eq!(parse_edid(&sample()[..10]), None);
    }

    #[test]
    fn the_core_name_needs_a_serial_otherwise_it_is_the_connector() {
        let e = parse_edid(&sample()).expect("edid");
        let id = OutputIdentity::from_edid("DP-1", Some(&e));
        assert_eq!(id.core_name(), "DEL DELL U2720Q ABC123");
        let mut no_serial = e.clone();
        no_serial.serial = None;
        assert_eq!(
            OutputIdentity::from_edid("DP-2", Some(&no_serial)).core_name(),
            "DP-2"
        );
        assert_eq!(OutputIdentity::connector_only("eDP-1").core_name(), "eDP-1");
    }

    #[test]
    fn the_order_is_make_model_serial_then_the_connector() {
        let a = OutputIdentity {
            connector: "DP-2".into(),
            make: Some("AAA".into()),
            model: Some("m".into()),
            serial: Some("1".into()),
        };
        let b = OutputIdentity {
            connector: "DP-1".into(),
            make: Some("BBB".into()),
            model: Some("m".into()),
            serial: Some("1".into()),
        };
        // 接続の名前では b が先だが、make で a が先。
        assert_eq!(a.compare(&b), Ordering::Less);
        // 識別が不明どうしは、接続の名前の順。
        let (x, y) = (
            OutputIdentity::connector_only("DP-1"),
            OutputIdentity::connector_only("DP-2"),
        );
        assert_eq!(x.compare(&y), Ordering::Less);
    }
}
