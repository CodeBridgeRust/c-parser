# c-parser 🦀

> *Парсер языка C с нулевым копированием на аренах и ленивой JSON-сериализацией*

<p align="center">
  <a href="README.md">🇬🇧 English</a> &nbsp;|&nbsp;
  <a href="README.ru.md">🇷🇺 Русский</a>
</p>

<p align="center">
  <a href="https://github.com/Fishman/c-parser/actions/workflows/ci.yml"><img src="https://github.com/Fishman/c-parser/actions/workflows/ci.yml/badge.svg" alt="Статус CI"></a>
  <a href="https://crates.io/crates/c-parser"><img src="https://img.shields.io/crates/v/c-parser.svg" alt="Версия на crates.io"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="Лицензия: GPL-3.0-or-later"></a>
</p>

---

## Обзор

`c-parser` выполняет лексический и синтаксический анализ исходного кода на C (C89–C17) и строит **AST на аренах** (arena-based), которое сериализуется в самодостаточный JSON (см. [`docs/ast-schema.json`](docs/ast-schema.json) v1.0). Крейт разработан как часть экосистемы `code-bridge`, но может использоваться самостоятельно.

**Ключевые особенности:**
- **Zero-copy лексер** — токены хранят только класс и `Span`; строковые данные остаются в исходном буфере.
- **AST на аренах** — узлы живут в плоском `Vec<Node>`, связаны 32-битными индексами `NodeId`. Вставка за O(1), пространственная локальность, стабильные индексы.
- **Ленивая материализация** — строковые поля (`name`, `value`, `op` и др.) извлекаются из исходного буфера только при JSON-сериализации через `String::from_utf8_lossy`.
- **Двухфазный парсинг** — рекурсивный спуск для объявлений/инструкций и **алгоритм Пратта** для выражений (O(n) линейное время, ограниченная глубина стека).
- **Инкрементальный режим** — обновление AST в памяти через отсортированный индекс `Span → NodeId` (O(log N + K)).
- **Восстановление после ошибок** — синтаксические ошибки порождают узлы `ErrorNode` в AST; парсер никогда не паникует на некорректном коде.

## Статус проекта

Все этапы разработки завершены. Релиз **v1.0.0** готов к публикации.

| Этап | Описание                                         | Статус |
|:----:|:-------------------------------------------------|:------:|
|   1  | Каркас рабочего пространства и документация      |   ✅   |
|   2  | Zero-copy лексер (BOM, комментарии, все токены)  |   ✅   |
|   3  | AST на аренах и JSON-сериализация                |   ✅   |
|   4  | Парсер: рекурсивный спуск + Пратт                |   ✅   |
|   5  | Инкрементальный режим (в памяти)                 |   ✅   |
|   6  | CLI-обёртка                                      |   ✅   |
|   7  | FFI-слой (JSON-only cdylib)                      |   ✅   |
|   8  | Примеры интеграции                               |   ✅   |
|   9  | Расширенный набор тестов                         |   ✅   |
|  10  | Бенчмарки и оптимизация                          |   ✅   |
|  11  | Финальная верификация и релиз                    |   ✅   |

- ✅ 282 теста проходят (лексер, AST, парсер, сериализация, инкрементальность, e2e CLI, безопасность FFI, валидация схемы, property-тесты, снапшоты)
- ✅ `cargo clippy --workspace --all-targets -- -D warnings` — без предупреждений
- ✅ `cargo fmt --all -- --check` — без отклонений
- ✅ Публичные типы парсера обладают трейтами `Send + Sync` (проверено на этапе компиляции)
- ✅ CI запускает `test`, `clippy`, `fmt` на Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64)
- ✅ Объявления `struct`/`union`/`enum`/`typedef`
- ✅ Определения функций с параметрами
- ✅ Все инструкции C (`if`, `while`, `do`-`while`, `for`, `switch`, `return`, `break`, `continue`, `goto`, метки)
- ✅ Разбор выражений по Пратту — 15 уровней приоритета, левая/правая ассоциативность
- ✅ Узлы директив препроцессора в режиме `SingleTu`
- ✅ Восстановление после ошибок с `ErrorNode` и массивом `errors[]`
- ✅ Публичное API через `create_parser` → трейт `ParserPlugin`
- ✅ JSON round-trip (`ast_to_json` / `ast_from_json`)

## Установка

### Как библиотека (Cargo)

Добавьте `c-parser` в `Cargo.toml`:

```toml
[dependencies]
c-parser = "1.0.0"
```

Либо зафиксируйте версию по тегу релиза для интеграции с `code-bridge`:

```toml
[dependencies]
c-parser = { git = "https://github.com/Fishman/c-parser", tag = "v1.0.0" }
```

### CLI (`c-parser-cli`) (например, в связке с генератором)

```bash
cargo install c-parser-cli
```

Либо сборка из исходников:

```bash
git clone https://github.com/Fishman/c-parser
cd c-parser
cargo install --path crates/c-parser-cli
```

## Использование

### Библиотека (Rust)

```rust
use c_parser::{create_parser, ast_to_json, ParseInput, ParserConfig};

fn parse_and_serialize(source: &[u8]) -> Result<String, c_parser::ParseError> {
    let parser = create_parser(ParserConfig::default());
    let result = parser.parse(ParseInput {
        source,
        filename: Some("example.c"),
        config: ParserConfig::default(),
    })?;

    let json = ast_to_json(&result.ast, source)?;
    Ok(json)
}
```

Инкрементальное обновление в памяти (сценарий LSP):

```rust
use c_parser::{incremental::IncrementalSession, ParseInput, ParserConfig, Span};

let source: Vec<u8> = std::fs::read("file.c")?;
let mut session = IncrementalSession::new(ParseInput {
    source: &source,
    filename: Some("file.c"),
    config: ParserConfig::default(),
})?;

// Заменить байты 42..47 на "+ 1"
session.apply_edit(Span { start: 42, end: 47 }, b"+ 1")?;
let json = session.to_json()?;
```

### CLI

```bash
# Полный разбор → отформатированный JSON в stdout
c-parser-cli parse example.c --pretty

# Сохранить в файл, выбрать диалект
c-parser-cli parse example.c --dialect c17 --pretty -o ast.json

# Сырая единица трансляции (директивы # сохраняются как узлы AST)
c-parser-cli parse example.c --preprocessing single-tu

# Инкрементальная правка: заменить байты 10..20
c-parser-cli parse-incremental example.c --start 10 --end 20
```

## Структура

```
c-parser/
├── .github/workflows/ci.yml      # CI: test + clippy + fmt на 5 платформенных целях
├── docs/                         # Архитектура, API-контракт, JSON-схема AST
│   ├── architecture.md
│   ├── api.md
│   ├── ast-schema.json
│   ├── parser-algorithm.md
│   ├── benchmarking.md
│   └── integration-with-code-bridge.md
├── crates/
│   ├── c-parser/                 # Основной библиотечный крейт
│   │   └── src/
│   │       ├── arena.rs          # Арена (Vec<Node>, NodeId = u32)
│   │       ├── ast/              # Типы AST: Node, NodeData, Span, TextSlice
│   │       ├── error.rs          # ParseError, Severity
│   │       ├── lexer/            # Zero-copy лексер C
│   │       ├── parser/           # Двухфазный парсер (рекурсивный спуск + Пратт)
│   │       ├── serialize/        # JSON-сериализация с ленивой материализацией
│   │       ├── index/            # Индекс Span→NodeId (поиск O(log N))
│   │       └── incremental/      # Инкрементальные обновления в памяти
│   ├── c-parser-cli/             # CLI-бинарник
│   └── c-parser-ffi/             # FFI cdylib (только JSON)
├── examples/                     # Примеры использования
├── tests/                        # Интеграционные, e2e, схема, property, снапшоты
├── README.md
└── README.ru.md
```

## Быстрый старт (разработка)

```bash
# Сборка рабочего пространства
cargo build --workspace

# Запуск всех тестов (282 теста)
cargo test --workspace

# Форматирование и линтинг
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings

# Запуск бенчмарков
cargo bench -p c-parser
```

## Документация

- [`docs/parser-algorithm.md`](docs/parser-algorithm.md) – гибридный алгоритм парсера (рекурсивный спуск + Пратт)
- [`docs/api.md`](docs/api.md) – полный API-контракт для интеграции с `code-bridge`
- [`docs/ast-schema.json`](docs/ast-schema.json) – JSON-схема AST v1.0 (Draft 2020-12)
- [`docs/architecture.md`](docs/architecture.md) – обзор архитектуры
- [`docs/benchmarking.md`](docs/benchmarking.md) – целевые показатели производительности и замеры
- [`docs/integration-with-code-bridge.md`](docs/integration-with-code-bridge.md) – инструкция по интеграции с `code-bridge`

## Совместимость

- **Rust:** stable ≥ 1.75
- **ОС:** Linux (x86_64, aarch64), macOS (x86_64, aarch64), Windows (x86_64)
- **Схема AST:** `docs/ast-schema.json` v1.0

## Лицензия

[GNU General Public License v3.0](LICENSE)
