# API-контракт крейта `c-parser` для интеграции в `code-bridge`

**Версия документа:** 1.2
**Статус:** Утверждён (согласован с ТЗ v2.2 и `docs/ast-schema.json` v1.0)
**Аудитория:** разработчик `code-bridge` (и иных потребителей), которому нужно писать интеграционный код, **не заглядывая в исходники `c-parser`**.
**Связанные документы:** `ТЗ v2.2`, `docs/ast-schema.json`, `docs/architecture.md`, `examples/embed_into_rust.rs`.

**История изменений:**
- v1.2 — финальная полировка: (1) §4.5 дополнен явным контрактом «`None` → либо отсутствие ключа, либо `null` — оба варианта валидны и десериализуются обратно в `None`»; (2) §5.4 снабжён пояснением о внутренней структуре `AstJson`.
- v1.1 — учтены замечания кросс-ревью: (1) явный контракт обработки `null` в опциональных `TextSlice`-полях + рекомендация снапшот-теста; (2) комментарий к `CallExpr::is_macro` о намеренном пропуске `None`; (3) явное требование к времени жизни `IncrementalSession` относительно исходного буфера.
- v1.0 — первая утверждённая версия.

---

## 1. Назначение и область применения

Крейт `c-parser` предоставляет:

- Публичный Rust-API (`create_parser`, трейт `ParserPlugin`, `parse`, `ast_to_json`, `ast_from_json`).
- Гарантированный JSON-контракт (схема `ast-schema.json` v1.0), полностью самодостаточный для генератора (все строковые поля материализованы).
- Опциональный FFI-слой (только JSON-строки через границу).

Всё, что **не** входит в контракт (внутренние структуры арены, `NodeId`, `Span`, `Arena`), **не должно** использоваться `code-bridge` в качестве типа-зависимости. Единственная точка связывания — крейт `c-parser` (статическая линковка) или его JSON-выход (динамическая загрузка / FFI).

---

## 2. Быстрый старт

```rust
use c_parser::{create_parser, ParserConfig, ParseInput, ast_to_json};

fn parse_and_serialize(source: &[u8]) -> anyhow::Result<String> {
    let parser = create_parser(ParserConfig::default());
    let result = parser.parse(ParseInput {
        source,
        filename: Some("example.c"),
        config: ParserConfig::default(),
    })?;

    // Сериализация в JSON. `source` обязателен — на нём работает lazy materialization.
    let json = ast_to_json(&result.ast, source)?;
    Ok(json)
}
```

Полный рабочий пример — `examples/embed_into_rust.rs` (см. §8).

---

## 3. Основные типы

Все типы ниже экспортируются из корня крейта: `pub use c_parser::*`. Обязательные трейты: `Send + Sync` у парсера, `serde::Serialize`/`Deserialize` у AST-типов (для `ast_from_json`).

### 3.1. `ParseInput<'a>`

```rust
pub struct ParseInput<'a> {
    /// Исходный буфер в байтах. НЕ обязательно UTF-8 (см. §5.5).
    pub source: &'a [u8],
    /// Используется только для диагностики; парсер его не материализует в AST.
    pub filename: Option<&'a str>,
    /// Конфигурация парсера (диалект, режим препроцессинга и т.д.).
    pub config: ParserConfig,
}
```

**Инварианты.** `source` живёт всё время парсинга и сериализации. `ParseInput` **не владеет** буфером. Для `IncrementalSession` — см. §6.2 (жёсткое требование к времени жизни).

### 3.2. `ParserConfig`

```rust
#[derive(Clone, Debug)]
pub struct ParserConfig {
    pub dialect: CDialect,
    pub preprocessing: PreprocessingMode,
    pub include_comments: bool,
    pub incremental: Option<IncrementalConfig>,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum CDialect {
    C89, C99, #[default] C11, C17,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum PreprocessingMode {
    /// Вход уже обработан препроцессором (`gcc -E`). Рекомендуется.
    #[default]
    Preprocessed,
    /// Вход — одна TU «как есть»; директивы `#` фиксируются как узлы AST без раскрытия.
    SingleTu,
}

#[derive(Clone, Debug, Default)]
pub struct IncrementalConfig {
    pub enabled: bool,
}
```

Соответствие CLI-опциям (`--dialect`, `--preprocessing`, `--include-comments`) — 1:1 (см. ТЗ §7.2). Значения по умолчанию: `C11`, `Preprocessed`, `false`, `None`.

### 3.3. `ParseResult`

```rust
pub struct ParseResult {
    /// AST. Содержит `errors` внутри (см. §4.7) — это необходимо
    /// для сериализации, т.к. схема требует массив `errors` на верхнем уровне.
    pub ast: Ast,
}

impl ParseResult {
    /// Удобный аксессор к `self.ast.errors` (для диагностики).
    pub fn errors(&self) -> &[ParseError] { &self.ast.errors }
}
```

**Важное отличие от черновика ТЗ.** В ТЗ `ParseResult` показан с отдельным полем `errors: Vec<ParseError>`. Поскольку схема (`ast-schema.json`) требует `errors` на верхнем уровне JSON **одновременно** с `nodes`, а `ErrorNode.error_index` ссылается на этот массив, `errors` хранятся внутри `Ast`. `ParseResult::errors()` — семантический аксессор.

### 3.4. `Ast`, `Node`, `Span`, `NodeId`

```rust
pub type NodeId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

pub struct Ast {
    pub root: NodeId,
    pub nodes: Vec<Node>,
    pub source_length: usize,
    /// Часть AST (см. §3.3). Массив нефатальных диагностик.
    pub errors: Vec<ParseError>,
}

pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
    pub children: Vec<NodeId>,
    /// `None` ⇔ `"data": null` в JSON (структурные узлы).
    pub data: Option<NodeData>,
}
```

**Инварианты, проверяемые парсером (не схемой):**
- `span.start <= span.end`;
- `span.end <= source_length`;
- `root` и каждый `children[i]` — валидный индекс в `nodes`;
- `ErrorNode.data.error_index < errors.len()`.

### 3.5. Трейт `ParserPlugin`

```rust
pub trait ParserPlugin: Send + Sync {
    /// Возвращает `"c"` для этого крейта. Поле `language` в JSON AST.
    fn language(&self) -> &'static str;

    /// Полный разбор. Возвращает `Err` только при фатальных ошибках
    /// (исчерпание ресурсов, некорректный span и т.п.). Синтаксические ошибки
    /// попадают в `ParseResult::errors()` и `ErrorNode` в AST.
    fn parse(&self, input: ParseInput<'_>) -> Result<ParseResult, ParseError>;

    /// Инкрементальное обновление in-memory. `change` — байтовый диапазон,
    /// в котором изменился `input.source` относительно текущего состояния
    /// парсера. См. §6.
    fn parse_incremental(
        &self,
        input: ParseInput<'_>,
        change: Span,
    ) -> Result<ParseResult, ParseError>;
}

/// Фабрика. Возвращает `Box<dyn ParserPlugin>` — ровно тот объект,
/// который регистрируется в `code-bridge` (см. ТЗ §7.4).
pub fn create_parser(config: ParserConfig) -> Box<dyn ParserPlugin>;
```

**Регистрация в `code-bridge`:**

```rust
registry.register(create_parser(ParserConfig::default()));
```

**Замечание по инкрементальности.** Сигнатура `parse_incremental(&self, …)` соответствует ТЗ, но требует, чтобы экземпляр парсера **удерживал** текущее состояние (AST + исходный буфер) между вызовами (интернали через `Mutex`/`RwLock` внутри реализации `ParserPlugin`). Это скрыто от потребителя. Для явного контроля над жизненным циклом сессии предоставляется обёртка `IncrementalSession` (см. §6.2) — рекомендуемый способ использования в `code-bridge`, если нужен контроль над владением буфером.

### 3.6. `ParseError`

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParseError {
    /// Стабильный код, e.g. "E_SYNTAX_EXPECTED_SEMICOLON".
    pub code: String,
    pub message: String,
    pub span: Span,
    pub severity: Severity,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity { Error, Warning, Note, Info }
```

**Сериализация.** `ParseError` → элемент массива `errors` в схеме (`required: code, message, span, severity`). `Severity` сериализуется в строку `"error" | "warning" | "note" | "info"`.

**Фатальность.** Ошибки уровня `Severity::Error` **не** делают `parse()` возвращающим `Err`; они попадают в `ast.errors` и в `ErrorNode`. `Err(ParseError)` — только для фатальных (например, `span` за пределами буфера).

---

## 4. Маппинг JSON-схемы на Rust-типы

Схема — источник истины. Ниже — соответствие один-в-один.

### 4.1. `NodeKind`

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    TranslationUnit, FunctionDefinition, VariableDeclaration, ParameterDeclaration,
    TypedefDeclaration, StructDeclaration, UnionDeclaration, EnumDeclaration, Enumerator,
    CompoundStatement, IfStatement, WhileStatement, DoWhileStatement, ForStatement,
    SwitchStatement, CaseStatement, DefaultStatement, ReturnStatement, BreakStatement,
    ContinueStatement, GotoStatement, LabelStatement, ExpressionStatement, NullStatement,
    Identifier, IntegerLiteral, FloatLiteral, StringLiteral, CharLiteral,
    BinaryExpr, UnaryExpr, AssignmentExpr, CallExpr, MemberExpr, IndexExpr,
    CastExpr, TernaryExpr, SizeofExpr, ParenExpr,
    TypeSpecifier, PointerType, ArrayType, FunctionType, InitializerList,
    PreprocessorInclude, PreprocessorDefine, PreprocessorUndef,
    PreprocessorIf, PreprocessorIfdef, PreprocessorIfndef,
    PreprocessorElse, PreprocessorElif, PreprocessorEndif,
    PreprocessorPragma, PreprocessorLine, PreprocessorError, PreprocessorWarning,
    ErrorNode,
}
```

Сериализуется как строка (вариант без переименования — `"TranslationUnit"` и т.д.).

### 4.2. `NodeData` — tagged union с `#[serde(tag = "type")]`

**Ключевое требование схемы:** поле `data.type` — это **не всегда** то же, что `kind`. Часть kind-ов использует сокращённый дискриминатор (`FunctionDefinition` → `FunctionDef`, `StructDeclaration` → `StructDef` и т.д. — полный список в `NodeKind` описании схемы).

Соответствие:

| `kind` | `data.type` | Rust-вариант `NodeData` |
|---|---|---|
| `Identifier` | `Identifier` | `Identifier { name: TextSlice }` |
| `IntegerLiteral` | `IntegerLiteral` | `IntegerLiteral { value: TextSlice, suffix: Option<TextSlice> }` |
| `FloatLiteral` | `FloatLiteral` | `FloatLiteral { value: TextSlice, suffix: Option<TextSlice> }` |
| `StringLiteral` | `StringLiteral` | `StringLiteral { value: TextSlice, encoding: Option<TextSlice> }` |
| `CharLiteral` | `CharLiteral` | `CharLiteral { value: TextSlice, encoding: Option<TextSlice> }` |
| `BinaryExpr` | `BinaryExpr` | `BinaryExpr { op: TextSlice }` |
| `UnaryExpr` | `UnaryExpr` | `UnaryExpr { op: TextSlice, prefix: bool }` |
| `AssignmentExpr` | `AssignmentExpr` | `AssignmentExpr { op: TextSlice }` |
| `CallExpr` | `CallExpr` | `CallExpr { is_macro: Option<bool> }` |
| `MemberExpr` | `MemberExpr` | `MemberExpr { op: TextSlice }` |
| `IndexExpr`, `CastExpr`, `TernaryExpr` | = kind | соответствующие варианты с полями |
| `SizeofExpr` | `SizeofExpr` | `SizeofExpr { is_variadic: bool }` |
| `FunctionDefinition` | **`FunctionDef`** | `FunctionDef { name: TextSlice, storage_class: TextSlice }` |
| `VariableDeclaration` | **`VariableDecl`** | `VariableDecl { name: TextSlice, storage_class: TextSlice }` |
| `ParameterDeclaration` | **`ParameterDecl`** | `ParameterDecl { name: TextSlice }` |
| `TypedefDeclaration` | **`TypedefDecl`** | `TypedefDecl { name: TextSlice }` |
| `StructDeclaration` | **`StructDef`** | `StructDef { name: Option<TextSlice> }` |
| `UnionDeclaration` | **`UnionDef`** | `UnionDef { name: Option<TextSlice> }` |
| `EnumDeclaration` | **`EnumDef`** | `EnumDef { name: Option<TextSlice> }` |
| `Enumerator` | `Enumerator` | `Enumerator { name: TextSlice, value: Option<TextSlice> }` |
| любой `Preprocessor*` | **`PreprocessorDirective`** | `PreprocessorDirective { directive: String, argument: TextSlice }` |
| `ErrorNode` | `ErrorNode` | `ErrorNode { error_index: u32 }` |

Rust-объявление:

```rust
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type")]
pub enum NodeData {
    Identifier { name: TextSlice },
    IntegerLiteral { value: TextSlice, #[serde(default, skip_serializing_if = "Option::is_none")] suffix: Option<TextSlice> },
    FloatLiteral { value: TextSlice, #[serde(default, skip_serializing_if = "Option::is_none")] suffix: Option<TextSlice> },
    StringLiteral { value: TextSlice, #[serde(default, skip_serializing_if = "Option::is_none")] encoding: Option<TextSlice> },
    CharLiteral { value: TextSlice, #[serde(default, skip_serializing_if = "Option::is_none")] encoding: Option<TextSlice> },
    BinaryExpr { op: TextSlice },
    UnaryExpr { op: TextSlice, prefix: bool },
    AssignmentExpr { op: TextSlice },
    CallExpr { #[serde(default, skip_serializing_if = "Option::is_none")] is_macro: Option<bool> },
    MemberExpr { op: TextSlice },
    IndexExpr,
    CastExpr,
    TernaryExpr,
    SizeofExpr { is_variadic: bool },

    #[serde(rename = "FunctionDef")]  FunctionDef { name: TextSlice, storage_class: TextSlice },
    #[serde(rename = "VariableDecl")] VariableDecl { name: TextSlice, storage_class: TextSlice },
    #[serde(rename = "ParameterDecl")] ParameterDecl { name: TextSlice },
    #[serde(rename = "TypedefDecl")] TypedefDecl { name: TextSlice },
    #[serde(rename = "StructDef")]    StructDef { name: Option<TextSlice> },
    #[serde(rename = "UnionDef")]     UnionDef { name: Option<TextSlice> },
    #[serde(rename = "EnumDef")]      EnumDef { name: Option<TextSlice> },
    Enumerator { name: TextSlice, #[serde(default, skip_serializing_if = "Option::is_none")] value: Option<TextSlice> },

    PreprocessorDirective {
        /// "include" | "define" | ... (без "#")
        directive: String,
        /// Всегда присутствует; пустая строка, если аргумент не задан.
        argument: TextSlice,
    },

    ErrorNode { error_index: u32 },
}
```

**Замечания.**
- `NodeData` **не** реализует `Serialize` через derive — сериализация выполняется вручную обёрткой `SerializableData` (см. §5), поскольку строковые поля `TextSlice` материализуются из исходного буфера. `Deserialize` derive присутствует и читает строки напрямую из JSON (в `TextSlice::Detached`).
- `#[serde(tag = "type")]` даёт `{"type": "Identifier", "name": "x"}`. `#[serde(rename = "FunctionDef")]` на варианте даёт `{"type": "FunctionDef", …}` для `kind: FunctionDefinition`. Это ровно то, что требует схема.
- Для `PreprocessorDirective.directive` используется `String` (не `TextSlice`), потому что это **не** исходный текст: имя директивы нормализуется парсером (`include`, `define`, …), и на него опирается cross-check `kind ↔ directive` в `tests/schema_validation.rs`.

### 4.3. `TextSlice` — внутреннее представление строк в AST

**Критически важно для понимания lazy materialization.** В AST строковые поля хранятся **не как `String`**, а как `TextSlice`:

```rust
#[derive(Clone, Debug)]
pub enum TextSlice {
    /// Ссылка на диапазон в исходном буфере. Используется парсером
    /// на этапе анализа (zero-copy). Материализуется ТОЛЬКО при сериализации.
    Span(Span),
    /// Уже материализованная строка. Используется только после `ast_from_json`.
    Detached(Box<str>),
}

impl TextSlice {
    pub fn materialize<'a>(&'a self, source: &'a [u8]) -> std::borrow::Cow<'a, str> {
        match self {
            TextSlice::Span(s) => String::from_utf8_lossy(&source[s.start..s.end]),
            TextSlice::Detached(s) => std::borrow::Cow::Borrowed(s),
        }
    }
}
```

- Парсер всегда создаёт `TextSlice::Span(..)` — **нулевое копирование**.
- `ast_from_json` создаёт `TextSlice::Detached(..)` — AST, отвязанный от исходного буфера (для генераторов, тестов, tooling-сценариев).
- `SerializableAst` (см. §5) умеет сериализовать оба варианта.

**Не сериализуется напрямую.** `TextSlice` не реализует `Serialize` (это привело бы к утечке `Span`-деталей в JSON). Материализация выполняется в `SerializableAst`.

### 4.4. Структурные узлы (`data: null`)

Для kind-ов без атрибутов (`CompoundStatement`, `IfStatement`, `WhileStatement`, `DoWhileStatement`, `ForStatement`, `SwitchStatement`, `CaseStatement`, `DefaultStatement`, `ReturnStatement`, `BreakStatement`, `ContinueStatement`, `GotoStatement`, `LabelStatement`, `ExpressionStatement`, `NullStatement`, `ParenExpr`, `TypeSpecifier`, `PointerType`, `ArrayType`, `FunctionType`, `InitializerList`, `TranslationUnit`) — `Node.data == None`, что сериализуется как `"data": null`.

Схема **требует** именно `null` (не отсутствие поля). `Option<NodeData>` с `None` даёт это автоматически.

### 4.5. Опциональные поля: `null`, отсутствие и `Option<TextSlice>`

В схеме `suffix`, `encoding`, `is_macro`, `Enumerator.value` помечены как необязательные (не в `required`) и допускают `["string", "null"]` (или `["boolean"]` для `is_macro`). В Rust им соответствуют `Option<T>`.

**Контракт сериализации (наш выход).** `None` в опциональных полях может сериализоваться **двумя способами**, и оба **валидны по схеме**:
- **Отсутствие ключа** — для `is_macro` (реализация: `#[serde(skip_serializing_if = "Option::is_none")]` в `NodeData::CallExpr` и ручной пропуск в `SerializableData::CallExpr`, см. §5.3). Семантика: «неизвестно / не отличено от `false`» — это осознанный tri-state контракт, зафиксированный в `ast-schema.json`.
- **Явный `null`** — для строковых опциональных полей `suffix`, `encoding`, `Enumerator.value` (реализация: `Option<Cow<str>>`, возвращаемый `SerializableData::opt_text`, сериализуется в `null` при `None`, см. §5.3). Семантика: «значения нет»; ключ всегда присутствует, потребителю не нужно проверять его наличие.

Разнообразие здесь **намеренное**: у `is_macro` трёхзначная семантика (`true` / `false` / «неизвестно»), у строковых полей — обычная двухзначная (`есть` / `нет`). Оба варианта покрыты схемой (`skip_serializing_if` на стороне `code-bridge` — опционален; главное, что `null` и отсутствие ключа трактуются одинаково).

**Контракт десериализации (вход от внешних потребителей):**
- Ключ отсутствует → `serde(default)` даёт `None`.
- Ключ `null` → `serde` **не вызывает** `TextSlice::deserialize`; `Option<TextSlice>::deserialize` сразу возвращает `None`. Это гарантируется стандартным поведением `serde` для `Option<T>` и не зависит от реализации `Deserialize` для `TextSlice`.
- Ключ со строкой → `TextSlice::deserialize` → `TextSlice::Detached`.

**Требование к потребителю (`code-bridge`).** Согласно `ast-schema.json`, отсутствие и `null` должны трактоваться **одинаково** (`None` в обоих случаях). Наш выход этому удовлетворяет (для строковых полей — всегда `null`; для `is_macro` — отсутствие ключа при `None`); вход от сторонних инструментов с `null` также принимается корректно.

**Тестовая гарантия (обязательно).** В `tests/schema_validation.rs` присутствует явный снапшот-тест `null_in_optional_fields_roundtrip`, который подаёт JSON с `"suffix": null`, `"encoding": null`, `"value": null` (в `Enumerator`), `"is_macro": null` и проверяет:
1. `ast_from_json` успешно десериализует документ (без паники кастомного `TextSlice::deserialize`).
2. `ast_to_json` для полученного `Ast` даёт JSON, валидный по схеме (поля с `None` на выходе либо отсутствуют, либо равны `null` — оба варианта допустимы по схеме).

Это защищает от регрессий при будущих изменениях в `serde` или рефакторинге `TextSlice`.

### 4.6. `ErrorNode`

```rust
Node {
    kind: NodeKind::ErrorNode,
    span: /* валидный span, охватывающий проблемный фрагмент */,
    children: /* индексы узлов, разобранных до ошибки */,
    data: Some(NodeData::ErrorNode { error_index: /* индекс в ast.errors */ }),
}
```

`ErrorNode` — полноценный связный узел; сериализация не падает; `code-bridge` подсвечивает ошибку по `errors[error_index].span`.

### 4.7. `errors` внутри `Ast`

Обоснование — §3.3. При сериализации `ast_to_json` кладёт `ast.errors` в верхнеуровневое поле `errors` JSON.

---

## 5. Паттерн `SerializableAst` (критический раздел)

### 5.1. Проблема, которую решает паттерн

Требования одновременно:
1. **Zero-copy** на этапе анализа: узлы хранят `TextSlice::Span`, не `String`.
2. **Самодостаточный JSON**: все строковые поля материализованы (генератор не имеет доступа к исходному буферу).
3. Использование стандартного `serde::Serialize`, у которого **нет** способа передать в `serialize()` дополнительный контекст (`source`).

`serde` не поддерживает «сериализацию с параметром». Решение — **обёртка**, удерживающая и AST, и `source`, реализующая `Serialize` вручную.

### 5.2. Структуры-обёртки

```rust
pub struct SerializableAst<'a> {
    pub ast: &'a Ast,
    pub source: &'a [u8],
}

// Внутренние обёртки
struct SerializableNodeArray<'a> { nodes: &'a [Node], source: &'a [u8] }
struct SerializableNode<'a>      { node: &'a Node,     source: &'a [u8] }
struct SerializableData<'a>      { data: &'a NodeData, source: &'a [u8] }
```

`SerializableAst` реализует `Serialize` и при обходе **материализует** `TextSlice` в `String::from_utf8_lossy`.

### 5.3. Полная реализация `Serialize`

```rust
use serde::ser::{Serialize, Serializer, SerializeMap, SerializeSeq};
use std::borrow::Cow;

// ---------- верхний уровень ----------
impl<'a> Serialize for SerializableAst<'a> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(6))?;
        map.serialize_entry("version", "1.0")?;
        map.serialize_entry("language", "c")?;
        map.serialize_entry("source_length", &self.ast.source_length)?;
        map.serialize_entry("root", &self.ast.root)?;
        map.serialize_entry(
            "nodes",
            &SerializableNodeArray { nodes: &self.ast.nodes, source: self.source },
        )?;
        // errors уже содержат готовые String; материализация не нужна.
        map.serialize_entry("errors", &self.ast.errors)?;
        map.end()
    }
}

// ---------- массив узлов ----------
impl<'a> Serialize for SerializableNodeArray<'a> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.nodes.len()))?;
        for node in self.nodes {
            seq.serialize_element(&SerializableNode { node, source: self.source })?;
        }
        seq.end()
    }
}

// ---------- один узел ----------
impl<'a> Serialize for SerializableNode<'a> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("kind", &self.node.kind)?;
        map.serialize_entry("span", &self.node.span)?;
        map.serialize_entry("children", &self.node.children)?;
        match &self.node.data {
            Some(data) => map.serialize_entry(
                "data",
                &SerializableData { data, source: self.source },
            )?,
            // ВАЖНО: именно `null`, не пропуск поля (см. §4.4).
            None => map.serialize_entry("data", &Option::<()>::None)?,
        }
        map.end()
    }
}

// ---------- data (tagged union, ручная материализация) ----------
impl<'a> SerializableData<'a> {
    fn text(&self, ts: &TextSlice) -> Cow<'a, str> {
        // Span срезается из исходного буфера; Detached уже готов.
        ts.materialize(self.source)
    }

    fn opt_text(&self, ts: &Option<TextSlice>) -> Option<Cow<'a, str>> {
        ts.as_ref().map(|t| self.text(t))
    }
}

impl<'a> Serialize for SerializableData<'a> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Нулевой хинт: число полей разное; передаём None — сериализатор сам разберётся.
        let mut map = serializer.serialize_map(None)?;
        match self.data {
            NodeData::Identifier { name } => {
                map.serialize_entry("type", "Identifier")?;
                map.serialize_entry("name", &self.text(name))?;
            }
            NodeData::IntegerLiteral { value, suffix } => {
                map.serialize_entry("type", "IntegerLiteral")?;
                map.serialize_entry("value", &self.text(value))?;
                // `None` сериализуется в `null` — это валидно по схеме (см. §4.5).
                map.serialize_entry("suffix", &self.opt_text(suffix))?;
            }
            NodeData::FloatLiteral { value, suffix } => {
                map.serialize_entry("type", "FloatLiteral")?;
                map.serialize_entry("value", &self.text(value))?;
                map.serialize_entry("suffix", &self.opt_text(suffix))?;
            }
            NodeData::StringLiteral { value, encoding } => {
                map.serialize_entry("type", "StringLiteral")?;
                map.serialize_entry("value", &self.text(value))?;
                map.serialize_entry("encoding", &self.opt_text(encoding))?;
            }
            NodeData::CharLiteral { value, encoding } => {
                map.serialize_entry("type", "CharLiteral")?;
                map.serialize_entry("value", &self.text(value))?;
                map.serialize_entry("encoding", &self.opt_text(encoding))?;
            }
            NodeData::BinaryExpr { op } => {
                map.serialize_entry("type", "BinaryExpr")?;
                map.serialize_entry("op", &self.text(op))?;
            }
            NodeData::UnaryExpr { op, prefix } => {
                map.serialize_entry("type", "UnaryExpr")?;
                map.serialize_entry("op", &self.text(op))?;
                map.serialize_entry("prefix", prefix)?;
            }
            NodeData::AssignmentExpr { op } => {
                map.serialize_entry("type", "AssignmentExpr")?;
                map.serialize_entry("op", &self.text(op))?;
            }
            NodeData::CallExpr { is_macro } => {
                map.serialize_entry("type", "CallExpr")?;
                // НАМЕРЕННО пропускаем сериализацию при None:
                // схема трактует отсутствие ключа как "unknown / likely plain call",
                // что совпадает с MVP-поведением (детекция макросов не реализована).
                // НЕ заменять на `is_macro.unwrap_or(false)` — это изменит семантику
                // (явный `false` вместо "неизвестно") и нарушит контракт DataCallExpr.
                if let Some(v) = is_macro {
                    map.serialize_entry("is_macro", v)?;
                }
            }
            NodeData::MemberExpr { op } => {
                map.serialize_entry("type", "MemberExpr")?;
                map.serialize_entry("op", &self.text(op))?;
            }
            NodeData::IndexExpr => { map.serialize_entry("type", "IndexExpr")?; }
            NodeData::CastExpr => { map.serialize_entry("type", "CastExpr")?; }
            NodeData::TernaryExpr => { map.serialize_entry("type", "TernaryExpr")?; }
            NodeData::SizeofExpr { is_variadic } => {
                map.serialize_entry("type", "SizeofExpr")?;
                map.serialize_entry("is_variadic", is_variadic)?;
            }

            NodeData::FunctionDef { name, storage_class } => {
                map.serialize_entry("type", "FunctionDef")?;
                map.serialize_entry("name", &self.text(name))?;
                map.serialize_entry("storage_class", &self.text(storage_class))?;
            }
            NodeData::VariableDecl { name, storage_class } => {
                map.serialize_entry("type", "VariableDecl")?;
                map.serialize_entry("name", &self.text(name))?;
                map.serialize_entry("storage_class", &self.text(storage_class))?;
            }
            NodeData::ParameterDecl { name } => {
                map.serialize_entry("type", "ParameterDecl")?;
                map.serialize_entry("name", &self.text(name))?;
            }
            NodeData::TypedefDecl { name } => {
                map.serialize_entry("type", "TypedefDecl")?;
                map.serialize_entry("name", &self.text(name))?;
            }
            NodeData::StructDef { name } => {
                map.serialize_entry("type", "StructDef")?;
                map.serialize_entry("name", &self.opt_text(name))?;
            }
            NodeData::UnionDef { name } => {
                map.serialize_entry("type", "UnionDef")?;
                map.serialize_entry("name", &self.opt_text(name))?;
            }
            NodeData::EnumDef { name } => {
                map.serialize_entry("type", "EnumDef")?;
                map.serialize_entry("name", &self.opt_text(name))?;
            }
            NodeData::Enumerator { name, value } => {
                map.serialize_entry("type", "Enumerator")?;
                map.serialize_entry("name", &self.text(name))?;
                map.serialize_entry("value", &self.opt_text(value))?;
            }

            NodeData::PreprocessorDirective { directive, argument } => {
                // Схема: directive — уже нормализованное имя (String).
                // argument — ВСЕГДА строка (возможно ""), материализуется.
                map.serialize_entry("type", "PreprocessorDirective")?;
                map.serialize_entry("directive", directive)?;
                map.serialize_entry("argument", &self.text(argument))?;
            }
            NodeData::ErrorNode { error_index } => {
                map.serialize_entry("type", "ErrorNode")?;
                map.serialize_entry("error_index", error_index)?;
            }
        }
        map.end()
    }
}
```

**Замечание по `opt_text` и `None`.** Для строковых опциональных полей (`suffix`, `encoding`, `value`) при `None` сериализуется **явный `null`** через `Option<Cow<str>>`, что схема разрешает (`type: ["string", "null"]`) и что удобнее для потребителей (ключ всегда есть — не нужно писать `if key in obj`). Для `is_macro` при `None` ключ **намеренно отсутствует** — см. комментарий в коде и §4.5.

### 5.4. `ast_to_json` и `ast_from_json`

```rust
pub fn ast_to_json(ast: &Ast, source: &[u8]) -> Result<String, serde_json::Error> {
    serde_json::to_string(&SerializableAst { ast, source })
}

pub fn ast_to_pretty_json(ast: &Ast, source: &[u8]) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&SerializableAst { ast, source })
}

pub fn ast_from_json(json: &str) -> Result<Ast, serde_json::Error> {
    // AstJson — внутренняя вспомогательная структура, полностью зеркалящая Ast,
    // но настроенная на корректную десериализацию в TextSlice::Detached.
    // Внешне (по полям JSON) AstJson неотличима от Ast; различие — на уровне
    // Rust-типов (например, поля с TextSlice десериализуются в Detached-вариант).
    // Span-ассоциация с исходным буфером не восстанавливается — это нормально:
    // генератор получает готовые строки.
    serde_json::from_str::<AstJson>(json).map(Ast::from_json_repr)
}
```

`TextSlice` поддерживает `Deserialize` только в форме `Detached`:

```rust
impl<'de> Deserialize<'de> for TextSlice {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Вызывается ТОЛЬКО когда JSON-значение — строка.
        // `null` и отсутствие ключа обрабатывает `Option<TextSlice>` выше,
        // возвращая `None` без захода сюда. См. §4.5.
        let s = String::deserialize(d)?;
        Ok(TextSlice::Detached(s.into_boxed_str()))
    }
}
```

**Требование к потребителю (`code-bridge`).** Предпочтительный сценарий — брать JSON-строку напрямую из `ast_to_json` и передавать генератору как есть. `ast_from_json` нужен, только если `code-bridge` **сам** хочет десериализовать AST в память Rust (например, для собственного обхода). При этом `TextSlice::Detached` далее не материализуется — `materialize` просто вернёт строку.

### 5.5. Обработка кодировок и невалидного UTF-8

`source` — это **`&[u8]`**, а не `&str`. Это осознанно (ТЗ §2.7, §5.4, §10):

- Legacy-код в Windows-1251 / KOI8-R и строковые литералы с байтами >0x7F — валидный вход.
- При материализации используется **`String::from_utf8_lossy`**:
  - валидные UTF-8 последовательности проходят без изменений;
  - невалидные байты → замена на `U+FFFD` (`�`);
  - **паники не бывает** даже на полностью бинарном буфере.

Это единственное место в крейте, где байты конвертируются в `String`. Тест `tests/schema_validation.rs::non_utf8_source_does_not_panic` покрывает этот сценарий.

**Что делать потребителю.** Если нужна точная семантика исходных байтов (например, для кодогенератора, копирующего литералы), `code-bridge` должен сохранить оригинальный `source` и при необходимости использовать `TextSlice::Span`-информацию. Но JSON-контракт **гарантирует** только lossy-материализованное представление — это задокументировано в `docs/ast-schema.json`.

### 5.6. Верификационный чеклист `SerializableAst`

Разработчик `code-bridge` может использовать этот список как приёмочный тест интеграции:

1. JSON, произведённый `ast_to_json`, проходит валидацию по `docs/ast-schema.json` (draft 2020-12).
2. Для любого листового узла (`Identifier`, `*Literal`, `PreprocessorDirective`) строковые поля — lossy-UTF-8 исходного текста в границах соответствующего `Span`.
3. Структурные kind-ы дают `"data": null`, а не отсутствие поля.
4. `PreprocessorDirective.argument` — `""` для `#endif`, `#else` и т.п.; не `null`, не отсутствует.
5. `ErrorNode.data.error_index` указывает на существующий элемент `errors`.
6. На файле с невалидным UTF-8 JSON собирается без паники, `errors` может содержать соответствующие диагностики.
7. `ast_from_json(ast_to_json(ast, src))` не падает (round-trip по структуре; `TextSlice` становится `Detached`).
8. **Входной JSON с `"suffix": null`, `"encoding": null`, `"value": null`, `"is_macro": null`** десериализуется `ast_from_json` без ошибок (см. §4.5, тест `null_in_optional_fields_roundtrip`).
9. `CallExpr` без `is_macro` в исходном AST сериализуется **без** ключа `is_macro` (а не с `"is_macro": false`). Потребитель трактует отсутствие как «неизвестно / эквивалентно `false`» (см. §4.5 и §5.3).

---

## 6. Инкрементальный режим

### 6.1. Контракт

- Работает **только in-memory** (ТЗ §2.5). Сериализация в JSON после обновления — по явному вызову.
- Сложность: **O(log N + K)** за счёт отсортированного индекса `Span → NodeId` (`src/index/`). «Сырой» бинарный поиск по `nodes` — **некорректен** (ТЗ §6.3).
- `change: Span` — диапазон в **новом** `input.source`, где произошло изменение относительно текущего состояния парсера.

### 6.2. Рекомендуемый способ для `code-bridge`: `IncrementalSession`

```rust
pub struct IncrementalSession<'a> { /* внутреннее состояние парсера */ }

impl<'a> IncrementalSession<'a> {
    /// ВАЖНО: `input.source` должен жить ДОЛЬШЕ сессии (`'a`).
    /// Сессия не копирует буфер; все `TextSlice::Span` внутри AST указывают
    /// в этот буфер. Если `code-bridge` планирует мутировать/перемещать буфер,
    /// он обязан либо клонировать его (`input.source.to_vec()` + владелец),
    /// либо гарантировать, что буфер живёт дольше сессии.
    pub fn new(input: ParseInput<'a>) -> Result<Self, ParseError>;

    /// Применить правку: `change` — диапазон в актуальном буфере,
    /// `new_text` — новый текст в этом диапазоне. Внутри обновляет AST
    /// и внутренний буфер.
    ///
    /// Замечание: так как сессия не владеет буфером, `apply_edit`
    /// принимает `new_text` отдельно; обновление AST выполняется
    /// по объединению старого буфера (до `change`) + `new_text` +
    /// старого буфера (после `change.end`), которое материализуется
    /// во внутреннем owned-буфере сессии.
    pub fn apply_edit(&mut self, change: Span, new_text: &[u8]) -> Result<(), ParseError>;

    pub fn ast(&self) -> &Ast;
    pub fn source(&self) -> &[u8];
    pub fn errors(&self) -> &[ParseError];
}

impl<'a> IncrementalSession<'a> {
    /// Удобная обёртка над `ast_to_json` (материализация по текущему буферу).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        ast_to_json(self.ast(), self.source())
    }
}
```

**Время жизни — контрактное требование.** Параметр `'a` в `IncrementalSession<'a>` — это время жизни исходного буфера, переданного в `new()`. Сессия **не копирует** буфер при создании (zero-copy), поэтому:

- `source` должен жить **дольше** сессии. Компилятор Rust обеспечит это на уровне типов; `code-bridge` не должен пытаться обойти ограничение через `unsafe`/`transmute`.
- Если `code-bridge` хранит буфер в `String`/`Vec<u8>` и планирует его **мутировать** (что изменит адрес аллокации), он **обязан** либо:
  - держать сессию в том же скоупе, что и неизменяемый буфер (типичный LSP-сценарий);
  - либо клонировать буфер перед созданием сессии и владеть клоном параллельно с сессией;
  - либо использовать `parse_incremental` на `ParserPlugin` (см. §6.3) с внутренним owned-буфером.
- После первого `apply_edit` сессия создаёт **внутренний owned-буфер** (объединение старого фрагмента + `new_text` + хвоста); последующие вызовы `source()` возвращают уже этот буфер. Это деталь реализации, не влияющая на контракт `'a` для **исходного** буфера.

**Пример типичного LSP-сценария (вне крейта, в `code-bridge`):**

```rust
// Буфер живёт в скоупе, пока жива сессия.
let original_bytes: Vec<u8> = read_file(path)?;
let mut session = IncrementalSession::new(ParseInput {
    source: &original_bytes,
    filename: Some("file.c"),
    config: ParserConfig::default(),
})?;

// Пришло изменение из LSP: заменить байты [42, 47) на "+ 1".
session.apply_edit(Span { start: 42, end: 47 }, b"+ 1")?;

// По запросу — сгенерировать полный JSON для транспиляции.
let json = session.to_json()?;
```

**Альтернатива при мутируемом буфере.** Если `code-bridge` использует `rope`/`text_buffer` с частыми правками, он может держать «снапшот» байтов на момент создания сессии и создавать новую сессию при существенных изменениях — либо использовать `ParserPlugin::parse_incremental`, у которого буфером владеет сам парсер (см. §6.3).

### 6.3. Трейт-метод `parse_incremental`

Для интеграции через `ParserPlugin` (ТЗ §7.1) доступен и метод `parse_incremental(&self, input, change)`. Он применим, если `code-bridge` использует **один экземпляр** `Box<dyn ParserPlugin>` с внутренним состоянием. Семантика эквивалентна `IncrementalSession::apply_edit`, но без явного владения буфером; реализация копирует `input.source` во внутренний owned-буфер при первом вызове и далее работает с ним.

```rust
// Псевдокод consumer-а:
let parser = create_parser(ParserConfig::default());
parser.parse(initial_input)?;                     // инициализирует состояние
let result = parser.parse_incremental(new_input, change)?;
let json = ast_to_json(&result.ast, new_input.source)?;
```

**Рекомендация.** Для `code-bridge` предпочтителен `IncrementalSession`: явный жизненный цикл, отсутствие скрытого `Mutex`, проще тестировать. `parse_incremental` у `ParserPlugin` оставлен для соответствия ТЗ и для случаев, когда `code-bridge` предпочитает единый интерфейс «плагин = объект с состоянием».

---

## 7. Обработка ошибок

### 7.1. Разделение

| Слой | Что | Куда |
|---|---|---|
| Лексические / синтаксические | Нефатальные | `ast.errors` + `ErrorNode` в `ast.nodes` |
| `ErrorNode.error_index` | Ссылка на `ast.errors[error_index]` | В `data` соответствующего `ErrorNode` |
| Фатальные (span out of range, OOM) | `Err(ParseError)` | `parse()` возвращает `Err` |
| Аргументы CLI | `exit code 2` | CLI (не библиотека) |

### 7.2. Упрощённый error recovery (MVP)

На синтаксической ошибке парсер:
1. Создаёт запись `ParseError` с `severity: error`;
2. Пропускает токены до ближайшей точки синхронизации (`;`, `}`, `{`);
3. Добавляет `ErrorNode` в арену с валидным `span` и `children` (узлы, построенные до ошибки);
4. Продолжает разбор.

**Контракт:** если `parse()` вернул `Ok`, AST гарантированно связный, `ast_to_json` не падает, `root` валиден, все `span.end <= source_length`.

### 7.3. Коды ошибок

Префиксы (стабильны, документируются в `src/error.rs`):
- `E_LEX_*` — лексические;
- `E_SYNTAX_*` — синтаксические;
- `E_PP_*` — препроцессорные (в режиме `single-tu`).

Пример: `E_SYNTAX_EXPECTED_SEMICOLON`, `E_SYNTAX_UNEXPECTED_TOKEN`.

---

## 8. Минимальный пример использования (`examples/embed_into_rust.rs`)

```rust
// examples/embed_into_rust.rs
//
// Демонстрация статической интеграции: создание парсера, вызов parse,
// обработка errors() и сериализация в JSON.
// Запуск: cargo run --example embed_into_rust -- path/to/file.c

use std::process::ExitCode;

use c_parser::{ast_to_json, create_parser, ParseInput, ParserConfig};

fn main() -> ExitCode {
    let path = match std::env::args().nth(1) {
        Some(path) => path,
        None => {
            eprintln!("usage: cargo run --example embed_into_rust -- <file.c>");
            return ExitCode::from(2);
        }
    };

    // Источник читается как байты — не обязан быть валидным UTF-8.
    let source = std::fs::read(&path).unwrap_or_else(|err| {
        eprintln!("error: cannot read `{path}`: {err}");
        std::process::exit(1);
    });

    // 1. Создание парсера (конфигурация по умолчанию).
    let config = ParserConfig::default();
    let parser = create_parser(config.clone());

    // 2. Парсинг. `Err` — только фатальная ошибка; нефатальные синтаксические
    //    ошибки попадают в `result.errors()` (см. шаг 3).
    let result = parser.parse(ParseInput {
        source: &source,
        filename: Some(&path),
        config,
    }).unwrap_or_else(|err| {
        eprintln!("error: fatal parse error: {err}");
        std::process::exit(1);
    });

    // 3. Диагностика — отдельно от AST.
    for err in result.errors() {
        eprintln!("{}:{}: {} ({}..{})",
            path, err.code, err.message, err.span.start, err.span.end);
    }

    // 4. JSON самодостаточен; `source` нужен для ленивой материализации.
    match ast_to_json(&result.ast, &source) {
        Ok(json) => println!("{json}"),
        Err(err) => {
            eprintln!("error: JSON serialization failed: {err}");
            return ExitCode::from(1);
        }
    }

    ExitCode::SUCCESS
}
```

**Сценарий `code-bridge`** (статическая линковка, ТЗ §7.4):

```toml
# Cargo.toml code-bridge
[dependencies]
c-parser = { git = "https://…/c-parser", tag = "v1.0.0" }
```

```rust
// Регистрация плагина.
use c_parser::{create_parser, ParserConfig};
registry.register(create_parser(ParserConfig::default()));

// Вызов.
let result = parser.parse(ParseInput { source: &bytes, filename, config })?;
let json = c_parser::ast_to_json(&result.ast, &bytes)?;
generator.feed(&json);
```

**Сценарий FFI** (динамическая загрузка, ТЗ §7.3). Граница — **только JSON-строки**. Из C/C++:

```c
const uint8_t* src = /* ... */;
size_t src_len = /* ... */;
const char* cfg_json = "{}";   // зарезервировано; сейчас игнорируется, но должно быть валидным JSON
char* out = c_parser_parse(src, src_len, (const uint8_t*)cfg_json, strlen(cfg_json));
if (out) {
    /* использовать out как JSON-строку */
    c_parser_free_string(out);       // ОБЯЗАТЕЛЬНО: освобождает аллокатор Rust
}
```

**Правило памяти.** Строка, выделенная `c_parser_parse`, освобождается только `c_parser_free_string`. Смешивать с `free()`/`delete` **нельзя** (ТЗ §7.3). Сложные типы (`Ast`, `Vec<Node>`, `Arena`) через границу **не пересекаются** — это гарантирует стабильность ABI.

---

## 9. Чеклист для разработчика `code-bridge`

Минимально необходимые шаги для полной интеграции:

1. **Добавить зависимость** `c-parser` в `Cargo.toml` (статическая линковка) **или** собрать `c-parser-ffi` как `cdylib` и загрузить через `libloading` (динамическая).
2. **Сконфигурировать `ParserConfig`**: `dialect` (по умолчанию `C11`), `preprocessing` (`Preprocessed` — рекомендуется), `include_comments` (обычно `false`), `incremental` (`enabled: false` для первого внедрения).
3. **Реализовать регистрацию** в реестре плагинов через `create_parser(config)`.
4. **Вызывать `parse(ParseInput { … })`**; при `Ok` — брать `result.ast`.
5. **Сериализовать** через `ast_to_json(&result.ast, &source)`; передавать JSON генератору **как есть**.
6. **Обрабатывать `result.errors()`** отдельно от AST (диагностика, подсветка).
7. **Проверить JSON** на соответствие `docs/ast-schema.json` (в CI `code-bridge`).
8. **Проверить non-UTF-8 источник**: `ast_to_json` не должен паниковать; проверить наличие `�` в материализованных строках.
9. **Проверить `ErrorNode`**: сгенерировать заведомо битый вход, убедиться, что `errors` непуст, `ErrorNode.data.error_index` валиден, JSON собирается.
10. **Проверить опциональные поля с `null`**: подать JSON с `"suffix": null`, `"encoding": null`, `"value": null`, `"is_macro": null` в `ast_from_json` — не должно быть ошибок (см. §4.5). На своём выходе `code-bridge` не должен полагаться на то, что `is_macro` всегда присутствует.
11. **Для инкрементального режима** — использовать `IncrementalSession` (§6.2), обеспечив время жизни буфера (`source` живёт дольше сессии) либо используя `parse_incremental` на `ParserPlugin` (§6.3). Не забыть, что JSON генерируется **по запросу** (не автоматически после каждого edit).
12. **Для FFI** — проверить парность `c_parser_parse` / `c_parser_free_string` и отсутствие утечек (valgrind/ASan на стороне C-вызывающего).
13. **Контракт совместимости**: `version` JSON (`"1.0"`) должен совпадать с ожиданием `code-bridge`. При мажорном расхождении — ошибка интеграции (ТЗ §7.4).

---

## 10. Ссылки

- `docs/ast-schema.json` — нормативная JSON-схема AST v1.0.
- `docs/architecture.md` — устройство арены, лексера, парсера.
- `docs/parser-algorithm.md` — рекурсивный спуск + Pratt, разрешение `typedef`-неоднозначности.
- `docs/integration-with-code-bridge.md` — пошаговая инструкция для `code-bridge`.
- `examples/embed_into_rust.rs` — рабочий пример из §8.
- `tests/schema_validation.rs` — эталон валидации JSON (в т.ч. non-UTF-8, `ErrorNode`-связность, `null_in_optional_fields_roundtrip`).
- `tests/cli_e2e.rs` — эталон вызовов CLI.
- ТЗ v2.2, §7 — исходные требования к API, FFI и интеграции.
