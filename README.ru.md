# c-parser 🦀

> *Парсер языка C с нулевым копированием на аренах и ленивой JSON-сериализацией*

<p align="center">
  <a href="README.md">🇬🇧 English</a> &nbsp;|&nbsp;
  <a href="README.ru.md">🇷🇺 Русский</a>
</p>

---

## Обзор

`c-parser` выполняет лексический и синтаксический анализ исходного кода на C (C89–C17) и строит **AST на аренах** (arena-based), которое сериализуется в самодостаточный JSON (см. [`docs/ast-schema.json`](docs/ast-schema.json) v1.0). Крейт разработан как часть экосистемы `code-bridge`, но может использоваться самостоятельно.

**Ключевые особенности:**
- **Zero-copy лексер** — токены хранят только класс и `Span`; строковые данные остаются в исходном буфере.
- **AST на аренах** — узлы живут в плоском `Vec<Node>`, связаны 32-битными индексами `NodeId`. Вставка за O(1), пространственная локальность, стабильные индексы.
- **Ленивая материализация** — строковые поля (`name`, `value`, `op` и др.) извлекаются из исходного буфера только при JSON-сериализации через `String::from_utf8_lossy`.
- **Двухфазный парсинг** — рекурсивный спуск для объявлений/инструкций и **алгоритм Пратта** для выражений (O(n) линейное время, ограниченная глубина стека).
- **Восстановление после ошибок** — синтаксические ошибки порождают узлы `ErrorNode` в AST; парсер никогда не паникует на некорректном коде.

## Статус проекта

| Этап | Описание                                         | Статус |
|:----:|:-------------------------------------------------|:------:|
|   1  | Каркас рабочего пространства и документация      |   ✅   |
|   2  | Zero-copy лексер (BOM, комментарии, все токены)  |   ✅   |
|   3  | AST на аренах и JSON-сериализация                |   ✅   |
|   4  | **Парсер: рекурсивный спуск + Пратт**            |   ✅   |
|   5  | Инкрементальный режим (в памяти)                 |   [ ]  |
|   6  | CLI-обёртка                                      |   [ ]  |
|   7  | FFI-слой                                         |   [ ]  |
|   8  | Примеры интеграции                               |   [ ]  |
|   9  | Расширенный набор тестов                         |   [ ]  |
|  10  | Бенчмарки и оптимизация                          |   [ ]  |
|  11  | Финальная верификация и релиз                    |   [ ]  |

### Реализовано в Этапе 4

| Компонент                 | Файл                                     | Назначение                                                |
|:--------------------------|:-----------------------------------------|:----------------------------------------------------------|
| Таблица символов          | `src/parser/symbol_table.rs`             | Стековая таблица для разрешения `typedef`/`identifier`    |
| Ошибки парсера            | `src/parser/errors.rs`                   | Коды ошибок `E_SYNTAX_*`                                  |
| Ядро парсера              | `src/parser/mod.rs`                      | Поток токенов, мост к арене, восстановление после ошибок  |
| Фаза A (объявления/инстр.)| `src/parser/phase_a.rs`                  | Рекурсивный спуск — функции, struct, `if`, `for`...       |
| Фаза B (выражения)        | `src/parser/phase_b.rs`                  | Алгоритм Пратта — все операторы C, O(n)                   |

- ✅ 95 тестов проходят (лексер, AST, парсер, сериализация)
- ✅ Объявления `struct`/`union`/`enum`/`typedef`
- ✅ Определения функций с параметрами
- ✅ Все инструкции C (`if`, `while`, `do`-`while`, `for`, `switch`, `return`, `break`, `continue`, `goto`, метки)
- ✅ Разбор выражений по Пратту — 15 уровней приоритета, левая/правая ассоциативность
- ✅ Узлы директив препроцессора в режиме `SingleTu`
- ✅ Восстановление после ошибок с `ErrorNode` и массивом `errors[]`
- ✅ Публичное API через `create_parser` → трейт `ParserPlugin`
- ✅ JSON round-trip (`ast_to_json` / `ast_from_json`)

## Структура

```
c-parser/
├── docs/                          # Архитектура, API-контракт, JSON-схема AST
│   ├── architecture.md
│   ├── api.md
│   ├── ast-schema.json
│   └── parser-algorithm.md
├── crates/
│   ├── c-parser/                  # Основной библиотечный крейт
│   │   └── src/
│   │       ├── arena.rs           # Арена (Vec<Node>, NodeId = u32)
│   │       ├── ast/               # Типы AST: Node, NodeData, Span, TextSlice
│   │       ├── error.rs           # ParseError, Severity
│   │       ├── lexer/             # Zero-copy лексер C
│   │       ├── parser/            # Двухфазный парсер
│   │       │   ├── mod.rs         # Структура Parser и поток токенов
│   │       │   ├── phase_a.rs     # Рекурсивный спуск
│   │       │   ├── phase_b.rs     # Алгоритм Пратта
│   │       │   ├── symbol_table.rs
│   │       │   └── errors.rs
│   │       ├── serialize/         # JSON-сериализация с ленивой материализацией
│   │       ├── index/             # Индекс Span→NodeId (Этап 5)
│   │       └── incremental/       # Инкрементальные обновления (Этап 5)
│   ├── c-parser-cli/              # CLI-бинарник (Этап 6)
│   └── c-parser-ffi/              # FFI cdylib (Этап 7)
├── examples/                      # Примеры использования
├── README.md
└── README.ru.md
```

## Быстрый старт

```bash
# Сборка библиотеки
cargo build -p c-parser

# Запуск тестов (95 тестов)
cargo test -p c-parser

# Форматирование и линтинг
cargo fmt --all
cargo clippy --workspace
```

## Rust API

```rust
use c_parser::{create_parser, ast_to_json, ParseInput, ParserConfig};

fn parse_and_serialize(source: &[u8]) -> anyhow::Result<String> {
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

## Документация

- [`docs/parser-algorithm.md`](docs/parser-algorithm.md) – гибридный алгоритм парсера (рекурсивный спуск + Пратт)
- [`docs/api.md`](docs/api.md) – полный API-контракт для интеграции с `code-bridge`
- [`docs/ast-schema.json`](docs/ast-schema.json) – JSON-схема AST v1.0 (Draft 2020-12)
- [`docs/architecture.md`](docs/architecture.md) – обзор архитектуры

## Лицензия

[GNU General Public License v3.0](LICENSE)
