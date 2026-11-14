//! U4 段階 (a) の手順 0: CDL 0.2.1 の挙動の確認（計画 §13 の未確認）。Contrast の `Env` と同じ形の型で、
//! (1) 関数形ルートと分割したファイルの import、(2) 省略できるフィールド（`focus?`・`workspace.name?`）の渡し方、
//! (3) 入れ子の誤りの E4014 のメッセージ（経路）を確かめる。

use std::collections::BTreeMap;

use cdl_host::{
    ComponentDef, Element, Engine, Field, Literal, ModuleResolver, Node, Program, Schema, Source,
    Ty, Value,
};

const M: &str = "host:t";

/// 文字列リテラルのユニオン。
fn one_of(values: &[&str]) -> Ty {
    Ty::union(
        values
            .iter()
            .map(|v| Ty::Literal(Literal::String((*v).to_owned())))
            .collect(),
    )
}

/// Contrast の `Env`（ui-spec §4.3）と同じ形。
fn env_ty() -> Ty {
    Ty::object([
        ("profile", Field::required(Ty::String)),
        ("mode", Field::required(Ty::String)),
        (
            "output",
            Field::required(Ty::object([("name", Field::required(Ty::String))])),
        ),
        (
            "workspace",
            Field::required(Ty::object([
                ("index", Field::required(Ty::Number)),
                ("name", Field::optional(Ty::String)),
                ("windowCount", Field::required(Ty::Number)),
            ])),
        ),
        (
            "focus",
            Field::optional(Ty::object([
                ("appId", Field::required(Ty::String)),
                ("isDialog", Field::required(Ty::Boolean)),
                (
                    "state",
                    Field::required(one_of(&["tiled", "floating", "fullscreen"])),
                ),
            ])),
        ),
    ])
}

fn schema() -> Result<Schema, String> {
    Schema::builder()
        .root(M, "Root")
        .env(M, env_ty())
        .component(M, ComponentDef::new("Root").children(&["Item"]))
        .component(M, ComponentDef::new("Item").prop("name", Ty::String))
        .build()
        .map_err(|e| format!("{e:?}"))
}

struct Memory(BTreeMap<&'static str, &'static str>);

impl ModuleResolver for Memory {
    fn resolve(&self, _importer: Option<&str>, specifier: &str) -> Result<Source, String> {
        let id = specifier.trim_start_matches("./");
        self.0
            .get(id)
            .map(|t| Source {
                id: id.to_owned(),
                text: (*t).to_owned(),
            })
            .ok_or_else(|| "なし".to_owned())
    }
}

fn load(files: &[(&'static str, &'static str)]) -> Result<Program, String> {
    let files = Memory(files.iter().copied().collect());
    Engine::new()
        .with_schema(schema()?)
        .load(&files, "main.cdl")
        .map_err(|d| {
            d.iter()
                .map(|d| format!("{}:{} {}", d.span.file, d.code.as_str(), d.message))
                .collect::<Vec<_>>()
                .join("\n")
        })
}

fn base_env(focus: Option<Value>, ws_name: Option<&str>) -> Value {
    let mut ws = vec![
        ("index", Value::Number(1.0)),
        ("windowCount", Value::Number(0.0)),
    ];
    if let Some(n) = ws_name {
        ws.push(("name", Value::string(n)));
    }
    let mut fields = vec![
        ("profile", Value::string("default")),
        ("mode", Value::string("normal")),
        ("output", Value::object([("name", Value::string("DP-1"))])),
        ("workspace", Value::object(ws)),
    ];
    if let Some(f) = focus {
        fields.push(("focus", f));
    }
    Value::object(fields)
}

fn focus(app_id: &str, state: &str) -> Value {
    Value::object([
        ("appId", Value::string(app_id)),
        ("isDialog", Value::Bool(false)),
        ("state", Value::string(state)),
    ])
}

fn names(tree: &Element) -> Vec<String> {
    Node::new(tree)
        .children_named("Item")
        .filter_map(|c| c.prop::<String>("name").ok())
        .collect()
}

// `config/` に似た分割: 入口が関数形ルート、条件（述語）を 2 つのファイルから import。
// 型の制約（実測）: `&&` の左辺は `boolean`。オブジェクト型 | undefined は `=== undefined` で比べられない（E3006）ので、
// 省略できるフィールドは `?.`（`env.focus?.appId === "foot"`）で読む。
// `c && <Item/>` は `false | Element` なので、配列に入れず子の位置に直接書く（E3016）。
const MAIN: &str = "\"use cdl 1\";\n\
    import { Root, Item } from \"host:t\";\n\
    import type { Env } from \"host:t\";\n\
    import { isDim, isFoot, isFull, inWeb } from \"./profiles.cdl\";\n\
    import { isResize } from \"./modes.cdl\";\n\
    export default function App(env: Env): Element<Root> {\n\
    \x20 return (\n\
    \x20   <Root>\n\
    \x20     {isDim(env) && <Item name=\"dim\" />}\n\
    \x20     {isFoot(env) && <Item name=\"foot\" />}\n\
    \x20     {isFull(env) && <Item name=\"full\" />}\n\
    \x20     {inWeb(env) && <Item name=\"web\" />}\n\
    \x20     {isResize(env) && <Item name=\"resize\" />}\n\
    \x20   </Root>\n\
    \x20 );\n\
    }\n";
const PROFILES: &str = "\"use cdl 1\";\n\
    import type { Env } from \"host:t\";\n\
    export function isDim(env: Env): boolean {\n\
    \x20 return env.profile === \"dim\";\n\
    }\n\
    export function isFoot(env: Env): boolean {\n\
    \x20 return env.focus?.appId === \"foot\";\n\
    }\n\
    export function isFull(env: Env): boolean {\n\
    \x20 return env.focus?.state === \"fullscreen\";\n\
    }\n\
    export function inWeb(env: Env): boolean {\n\
    \x20 return env.workspace.name === \"web\";\n\
    }\n";
const MODES: &str = "\"use cdl 1\";\n\
    import type { Env } from \"host:t\";\n\
    export function isResize(env: Env): boolean {\n\
    \x20 return env.mode === \"resize\";\n\
    }\n";

#[test]
fn a_function_root_split_over_three_files_renders_per_env() -> Result<(), String> {
    let p = load(&[
        ("main.cdl", MAIN),
        ("profiles.cdl", PROFILES),
        ("modes.cdl", MODES),
    ])?;
    assert!(p.is_function_root());
    let render = |env: &Value| {
        p.render(env)
            .map(|t| names(&t))
            .map_err(|d| format!("{d:?}"))
    };
    assert_eq!(render(&base_env(None, None))?, Vec::<String>::new());
    assert_eq!(
        render(&base_env(Some(focus("foot", "fullscreen")), Some("web")))?,
        ["foot", "full", "web"]
    );
    let mut resize = base_env(None, None);
    if let Value::Object(map) = &mut resize {
        std::sync::Arc::make_mut(map).insert("mode".to_owned(), Value::string("resize"));
    }
    assert_eq!(render(&resize)?, ["resize"]);
    // 同じ入力から同じ木（純粋・決定的）。
    let again = render(&base_env(Some(focus("foot", "fullscreen")), Some("web")))?;
    assert_eq!(again, ["foot", "full", "web"]);
    Ok(())
}

#[test]
fn optional_fields_may_be_omitted_and_an_undefined_value_is_the_same_as_omitting()
-> Result<(), String> {
    let p = load(&[
        ("main.cdl", MAIN),
        ("profiles.cdl", PROFILES),
        ("modes.cdl", MODES),
    ])?;
    // 省略（フィールドを渡さない）。
    assert!(p.render(&base_env(None, None)).is_ok());
    // `undefined` を渡す。
    let mut with_undefined = vec![
        ("profile", Value::string("default")),
        ("mode", Value::string("normal")),
        ("output", Value::object([("name", Value::string("DP-1"))])),
        (
            "workspace",
            Value::object([
                ("index", Value::Number(1.0)),
                ("windowCount", Value::Number(0.0)),
                ("name", Value::Undefined),
            ]),
        ),
    ];
    with_undefined.push(("focus", Value::Undefined));
    let r = p.render(&Value::object(with_undefined));
    assert!(r.is_ok(), "{r:?}");
    Ok(())
}

#[test]
fn a_nested_mismatch_is_e4014_and_the_message_names_the_field() -> Result<(), String> {
    let p = load(&[
        ("main.cdl", MAIN),
        ("profiles.cdl", PROFILES),
        ("modes.cdl", MODES),
    ])?;
    let bad_state = base_env(Some(focus("foot", "maximized")), None);
    let errs = p.render(&bad_state).err().ok_or("誤りを期待した")?;
    assert_eq!(errs.len(), 1, "{errs:?}");
    let first = errs.first().ok_or("1 件")?;
    assert_eq!(first.code.as_str(), "E4014");
    assert!(
        first.message.contains("focus"),
        "合わないフィールドの経路が出る: {}",
        first.message
    );
    // 数であるべき所に文字列。
    let bad_count = Value::object([
        ("profile", Value::string("default")),
        ("mode", Value::string("normal")),
        ("output", Value::object([("name", Value::string("DP-1"))])),
        (
            "workspace",
            Value::object([
                ("index", Value::Number(1.0)),
                ("windowCount", Value::string("zero")),
            ]),
        ),
    ]);
    let errs = p.render(&bad_count).err().ok_or("誤りを期待した")?;
    let first = errs.first().ok_or("1 件")?;
    assert_eq!(first.code.as_str(), "E4014");
    assert!(
        first.message.contains("windowCount"),
        "入れ子の経路: {}",
        first.message
    );
    Ok(())
}
