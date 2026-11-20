# Contrast — cdl(DSL)で設定を宣言するWaylandコンポジタのサンドボックス実験

## 概要

- Rust(edition 2024) + Smithay によるWaylandコンポジタ
- 設定は [cdl](https://github.com/sh-sunfaded/cdl)(Contrast Document Language)で宣言する
- workspaceは5クレート: contrast-core / contrast-policy / contrast-policy-cdl / contrast-compositor / contrast
- 設計・仕様の詳細は docs/ を参照

## 必要環境

- Rust toolchain(`rust-toolchain.toml` で固定)
- Nixを使う場合は `nix develop`(`flake.nix` 同梱)

## ビルドとテスト

```sh
cargo build --workspace
cargo test --workspace
```

## 制限

- サンドボックス実験であり、実用は想定していない(詳細は docs/)

## ライセンス

MIT([LICENSE](LICENSE)を参照)。
