# Интеграция `c-parser` с `code-bridge`

**Версия документа:** 1.1  
**Статус:** Утверждён  
**Связанные документы:** ТЗ v2.2, API-контракт v1.2, `docs/ast-schema.json` v1.0

**История изменений:**
- v1.1 — учтены микро-правки кросс-ревью: (1) в §4.1 добавлен явный комментарий в код Rust/Go о том, что `config_json` обязан быть валидным JSON, несмотря на игнорирование в v1.0 (forward-compatibility); (2) в §7 добавлен отдельный пункт чеклиста о проверке десериализации `null` в опциональных полях (`"is_macro": null`, `"suffix": null` и т.п.) согласно API-контракту §4.5.
- v1.0 — первая утверждённая версия.

---

## 1. Введение

Настоящий документ описывает порядок интеграции парсера языка C (`c-parser`) в центральный модуль транспиляции `code-bridge`. Документ охватывает два сценария интеграции: статическую линковку (рекомендуемый способ) и динамическую загрузку через FFI-слой. Также описаны правила конфигурации и контракт версионирования JSON-схемы AST.

**Область применения:** документ предназначен для разработчиков `code-bridge`, выполняющих подключение парсера, и для специалистов по сопровождению, обеспечивающих совместимость версий.

**Границы контракта:** `code-bridge` не должен зависеть от внутренних типов `c-parser` (арена, `NodeId`, `Span`, `TextSlice`). Единственными точками связывания являются:
- публичный трейт `ParserPlugin` (при статической линковке);
- JSON-схема AST v1.0 (при любом способе интеграции);
- FFI-функции, работающие исключительно с JSON-строками (при динамической загрузке).

---

## 2. Способы интеграции

| Способ | Механизм | Рекомендация |
|--------|----------|--------------|
| Статическая линковка | Cargo-зависимость, прямой вызов Rust-API | **Основной**. Обеспечивает максимальную производительность и типобезопасность. |
| Динамическая загрузка (FFI) | `cdylib`, загрузка через `libloading`/`dlopen`, обмен только JSON-строками | Опциональный. Применяется при необходимости изоляции или использования из не-Rust языков. |

Оба способа используют единый JSON-контракт, что гарантирует совместимость результатов.

---

## 3. Сценарий 1: Статическая линковка

### 3.1. Добавление зависимости

В `Cargo.toml` проекта `code-bridge` добавьте зависимость от крейта `c-parser`:

```toml
[dependencies]
c-parser = { git = "https://github.com/your-org/c-parser", tag = "v1.0.0" }
```

Рекомендуется фиксировать точный тег версии, соответствующей требуемой версии JSON-схемы (см. раздел 6).

### 3.2. Регистрация парсера в реестре плагинов

`code-bridge` должен реализовать реестр плагинов, принимающий объекты, реализующие трейт `ParserPlugin`. Пример регистрации:

```rust
use c_parser::{create_parser, ParserConfig, ParserPlugin};

// Создание парсера с конфигурацией по умолчанию.
let config = ParserConfig::default();
let parser: Box<dyn ParserPlugin> = create_parser(config);

// Регистрация в реестре code-bridge.
registry.register(parser);
```

Если `code-bridge` поддерживает несколько языков, рекомендуется хранить парсеры в `HashMap<&'static str, Box<dyn ParserPlugin>>`, используя `parser.language()` в качестве ключа (`"c"` для данного крейта).

### 3.3. Вызов парсинга и сериализация в JSON

Типовой сценарий обработки одного файла:

```rust
use c_parser::{ParseInput, ast_to_json};
use anyhow::Result;

fn process_c_file(
    parser: &dyn ParserPlugin,
    source: &[u8],
    filename: &str,
) -> Result<String> {
    // 1. Подготовка входа.
    let input = ParseInput {
        source,
        filename: Some(filename),
        config: ParserConfig::default(),
    };

    // 2. Парсинг.
    let result = parser.parse(input)?;

    // 3. Диагностика (нефатальные ошибки) — отдельно от AST.
    for err in result.errors() {
        eprintln!(
            "{}:{}: {} ({}..{})",
            filename, err.code, err.message, err.span.start, err.span.end
        );
    }

    // 4. Сериализация AST в самодостаточный JSON.
    //    `source` обязателен для материализации строковых полей.
    let json = ast_to_json(&result.ast, source)?;

    Ok(json)
}
```

**Важно:** `ast_to_json` принимает исходный буфер `source`. Это необходимо для ленивой материализации строковых значений (идентификаторов, литералов) из `Span`-ов. Генератор получает готовый JSON, не требующий доступа к исходному файлу.

### 3.4. Обработка ошибок

- **Фатальные ошибки** (например, выход `span` за границы буфера, исчерпание памяти) возвращаются как `Err(ParseError)`. В этом случае парсинг следует прервать и передать ошибку в систему логирования `code-bridge`.
- **Нефатальные ошибки** (лексические, синтаксические) содержатся в `result.errors()` и в AST в виде узлов `ErrorNode`. Парсинг завершается успешно (`Ok`), JSON собирается, и `code-bridge` может передать его генератору, параллельно подсветив диагностику.
- При наличии `ErrorNode` JSON остаётся валидным по схеме; `ErrorNode.data.error_index` указывает на соответствующую запись в массиве `errors`.

### 3.5. Инкрементальный режим (кратко)

Для LSP-сценариев `code-bridge` может использовать `IncrementalSession` (см. API-контракт §6.2). Пример:

```rust
use c_parser::{IncrementalSession, ParseInput, ParserConfig, Span};

let source_bytes = std::fs::read("file.c")?;
let mut session = IncrementalSession::new(ParseInput {
    source: &source_bytes,
    filename: Some("file.c"),
    config: ParserConfig::default(),
})?;

// Применение правки: замена диапазона [42, 47) на "+ 1".
session.apply_edit(Span { start: 42, end: 47 }, b"+ 1")?;

// Генерация JSON по запросу (не автоматически).
let json = session.to_json()?;
```

**Требование к времени жизни:** исходный буфер `source_bytes` должен жить дольше сессии. Если `code-bridge` использует мутируемый буфер (rope), необходимо либо клонировать его перед созданием сессии, либо использовать `ParserPlugin::parse_incremental` (см. API-контракт §6.3).

---

## 4. Сценарий 2: Динамическая загрузка (FFI)

### 4.1. Сборка `c-parser-ffi`

Крейт `c-parser-ffi` собирается как `cdylib`:

```bash
cargo build --release -p c-parser-ffi
```

В результате создаётся:
- Linux: `libc_parser_ffi.so`
- macOS: `libc_parser_ffi.dylib`
- Windows: `c_parser_ffi.dll`

Библиотека экспортирует две функции:

```c
char* c_parser_parse(
    const uint8_t* source_ptr,
    size_t source_len,
    const uint8_t* config_json,
    size_t config_len
);

void c_parser_free_string(char* ptr);
```

`config_json` зарезервирован для будущих настроек; в v1.0 он игнорируется, но **должен быть валидным JSON** (например, `"{}"`).

### 4.2. Загрузка библиотеки и вызов функций (Rust)

```rust
use libloading::{Library, Symbol};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use anyhow::{anyhow, Result};

type CParseFn = unsafe extern "C" fn(
    *const u8, usize,
    *const u8, usize
) -> *mut c_char;

type CFreeFn = unsafe extern "C" fn(*mut c_char);

fn parse_via_ffi(source: &[u8], config_json: &str) -> Result<String> {
    unsafe {
        // 1. Загрузка библиотеки.
        let lib = Library::new("libc_parser_ffi.so")?;

        // 2. Получение символов.
        let c_parse: Symbol<CParseFn> = lib.get(b"c_parser_parse\0")?;
        let c_free: Symbol<CFreeFn> = lib.get(b"c_parser_free_string\0")?;

        // 3. Подготовка конфигурации.
        //    В v1.0 конфигурация игнорируется парсером, но строка обязана быть
        //    валидным JSON (например, "{}"), чтобы не сломать forward-compatibility:
        //    в будущих версиях парсер начнёт читать эту строку, и невалидный JSON
        //    приведёт к ошибке интеграции.
        let cfg = CString::new(config_json)?;
        let cfg_bytes = cfg.as_bytes();

        // 4. Вызов парсера.
        let out_ptr = c_parse(
            source.as_ptr(),
            source.len(),
            cfg_bytes.as_ptr(),
            cfg_bytes.len(),
        );

        if out_ptr.is_null() {
            return Err(anyhow!("c_parser_parse returned null"));
        }

        // 5. Чтение результата.
        let out_cstr = CStr::from_ptr(out_ptr);
        let json = out_cstr.to_str()?.to_owned();

        // 6. ОБЯЗАТЕЛЬНОЕ освобождение строки.
        c_free(out_ptr);

        Ok(json)
    }
}
```

### 4.3. Управление памятью на границе FFI

Функция `c_parser_parse` внутри `c-parser-ffi` создаёт `CString` и передаёт владение вызывающей стороне через `CString::into_raw`. Функция `c_parser_free_string` выполняет обратное преобразование:

```rust
#[no_mangle]
pub unsafe extern "C" fn c_parser_free_string(ptr: *mut c_char) {
    if ptr.is_null() { return; }
    unsafe {
        // Восстанавливаем CString и позволяем ему освободиться (drop).
        drop(CString::from_raw(ptr));
    }
}
```

**Критически важно:**
- Освобождать память необходимо **только** через `c_parser_free_string`. Использование `free()`, `delete` или иных средств освобождения приведёт к неопределённому поведению, так как память выделена аллокатором Rust.
- Парность `into_raw` / `from_raw` гарантирует, что память будет возвращена тому же аллокатору, который её выделил.
- Все сложные типы (`Ast`, `Vec<Node>`, `Arena`) остаются внутри библиотеки; через границу передаётся только `char*` с JSON-строкой.

### 4.4. Пример на C

```c
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dlfcn.h>
#include <stdint.h>

typedef char* (*c_parser_parse_t)(
    const uint8_t*, size_t,
    const uint8_t*, size_t
);
typedef void (*c_parser_free_string_t)(char*);

int main() {
    void* handle = dlopen("libc_parser_ffi.so", RTLD_LAZY);
    if (!handle) { fprintf(stderr, "dlopen: %s\n", dlerror()); return 1; }

    c_parser_parse_t parse = (c_parser_parse_t)dlsym(handle, "c_parser_parse");
    c_parser_free_string_t free_str = (c_parser_free_string_t)dlsym(handle, "c_parser_free_string");
    if (!parse || !free_str) { fprintf(stderr, "dlsym failed\n"); return 1; }

    const char* source = "int main() { return 0; }";
    const char* cfg = "{}";

    char* json = parse(
        (const uint8_t*)source, strlen(source),
        (const uint8_t*)cfg, strlen(cfg)
    );

    if (json) {
        printf("%s\n", json);
        free_str(json);   // ОБЯЗАТЕЛЬНО: освобождение через API парсера.
    } else {
        fprintf(stderr, "parse failed\n");
    }

    dlclose(handle);
    return 0;
}
```

### 4.5. Пример на Go (концептуально)

В Go для вызова функций из динамической библиотеки используется `cgo` и `dlopen`/`dlsym`. Из-за ограничений cgo вызов функции по указателю требует C-обёртки, но логика остаётся той же: загрузить библиотеку, получить символы, вызвать `c_parser_parse`, скопировать строку, вызвать `c_parser_free_string`.

```go
/*
#cgo LDFLAGS: -ldl
#include <stdlib.h>
#include <dlfcn.h>
#include <stdint.h>

typedef char* (*parse_fn)(const uint8_t*, size_t, const uint8_t*, size_t);
typedef void (*free_fn)(char*);

static char* call_parse(void* fn, const uint8_t* src, size_t src_len,
                        const uint8_t* cfg, size_t cfg_len) {
    return ((parse_fn)fn)(src, src_len, cfg, cfg_len);
}
static void call_free(void* fn, char* ptr) {
    ((free_fn)fn)(ptr);
}
*/
import "C"
import (
    "fmt"
    "unsafe"
)

func main() {
    handle := C.dlopen(C.CString("libc_parser_ffi.so"), C.RTLD_LAZY)
    if handle == nil { panic("dlopen failed") }
    defer C.dlclose(handle)

    parsePtr := C.dlsym(handle, C.CString("c_parser_parse"))
    freePtr := C.dlsym(handle, C.CString("c_parser_free_string"))

    source := []byte("int main() { return 0; }")
    // В v1.0 конфигурация игнорируется парсером, но строка обязана быть
    // валидным JSON (например, "{}"), чтобы не сломать forward-compatibility:
    // в будущих версиях парсер начнёт читать эту строку, и невалидный JSON
    // приведёт к ошибке интеграции.
    cfg := []byte("{}")

    jsonPtr := C.call_parse(
        parsePtr,
        (*C.uint8_t)(unsafe.Pointer(&source[0])), C.size_t(len(source)),
        (*C.uint8_t)(unsafe.Pointer(&cfg[0])), C.size_t(len(cfg)),
    )
    if jsonPtr != nil {
        fmt.Println(C.GoString(jsonPtr))
        C.call_free(freePtr, jsonPtr) // Обязательное освобождение.
    }
}
```

### 4.6. Безопасность и ограничения

- FFI-граница спроектирована исключительно для обмена JSON-строками. Не пытайтесь передавать через неё структуры `Ast`, `Vec<Node>` или использовать внутренние типы `c-parser` — это нарушит ABI-стабильность.
- Всегда проверяйте, что возвращённый указатель не `null`, и освобождайте его через `c_parser_free_string`.
- Убедитесь, что время жизни входного буфера `source` достаточно для завершения вызова `c_parser_parse` (функция не сохраняет указатель после возврата).
- Для сборки `c-parser-ffi` используйте ту же версию Rust, что и для `code-bridge`, во избежание несовместимости аллокаторов (рекомендуется системный аллокатор).

---

## 5. Конфигурация

### 5.1. Приоритет настроек

При запуске `code-bridge` применяется следующий приоритет (от высшего к низшему):

1. **Аргументы командной строки (CLI)** — явно переданные флаги.
2. **Переменные окружения** — `C_PARSER_*`.
3. **Файл конфигурации TOML** — путь задаётся через `C_PARSER_CONFIG` или аргумент `--config`.
4. **Значения по умолчанию** — определённые в `ParserConfig::default()`.

`code-bridge` должен реализовать слияние конфигураций в указанном порядке. Для статической линковки настройки транслируются в `ParserConfig`; для FFI — в JSON-строку `config_json` (в v1.0 зарезервировано, но должно быть валидным JSON).

### 5.2. Переменные окружения

| Переменная | Назначение | Допустимые значения |
|------------|------------|---------------------|
| `C_PARSER_LOG` | Уровень логирования | `error`, `warn`, `info`, `debug`, `trace` |
| `C_PARSER_CONFIG` | Путь к файлу конфигурации TOML | Путь к файлу |

### 5.3. Файл конфигурации TOML

```toml
[parser]
dialect = "c11"                     # c89 | c99 | c11 | c17
preprocessing = "preprocessed"      # preprocessed | single-tu
include_comments = false

[incremental]
enabled = false
```

### 5.4. Применение в `code-bridge`

Пример функции, собирающей `ParserConfig` с учётом приоритетов:

```rust
use c_parser::{ParserConfig, CDialect, PreprocessingMode, IncrementalConfig};

fn build_parser_config(
    cli_dialect: Option<CDialect>,
    cli_preprocessing: Option<PreprocessingMode>,
    cli_include_comments: Option<bool>,
    cli_incremental: Option<bool>,
) -> ParserConfig {
    let mut config = ParserConfig::default();

    // 1. Файл TOML (если задан).
    if let Ok(path) = std::env::var("C_PARSER_CONFIG") {
        if let Ok(toml_str) = std::fs::read_to_string(path) {
            // ... парсинг TOML и обновление config ...
        }
    }

    // 2. Переменные окружения (перекрывают TOML).
    if let Ok(val) = std::env::var("C_PARSER_DIALECT") {
        if let Some(d) = parse_dialect(&val) { config.dialect = d; }
    }
    // ... аналогично для других переменных ...

    // 3. CLI-аргументы (наивысший приоритет).
    if let Some(d) = cli_dialect { config.dialect = d; }
    if let Some(p) = cli_preprocessing { config.preprocessing = p; }
    if let Some(ic) = cli_include_comments { config.include_comments = ic; }
    if let Some(en) = cli_incremental {
        config.incremental = Some(IncrementalConfig { enabled: en });
    }

    config
}
```

### 5.5. Конфигурация в FFI

В текущей версии `config_json` игнорируется, но должен быть валидным JSON. Рекомендуется передавать `"{}"` или сериализованный объект с настройками (на будущее). Пример:

```json
{
  "dialect": "c11",
  "preprocessing": "preprocessed",
  "include_comments": false
}
```

`code-bridge` может формировать эту строку из тех же источников, что и для статической линковки.

---

## 6. Контракт версионирования JSON-схемы

### 6.1. Поле `version`

Корневой объект JSON AST содержит поле `version` (строка). Текущая версия схемы — `"1.0"`. Поле обязательно и не может отсутствовать.

### 6.2. Правила совместимости

- **Мажорная версия** (первое число) определяет несовместимые изменения. При несовпадении мажорной версии `code-bridge` должен отказаться от обработки JSON и выдать ошибку интеграции.
- **Минорная версия** (второе число) — добавление новых необязательных полей, новых значений `kind`/`data.type` или расширение допустимых значений. Обратная совместимость гарантируется: `code-bridge`, рассчитанный на версию 1.x, должен корректно обрабатывать JSON версии 1.y (y ≥ x), игнорируя неизвестные поля.
- **Патч-версия** — исправления документации или схемы без изменения структуры данных.

### 6.3. Валидация

`code-bridge` обязан проверять версию перед десериализацией или передачей JSON генератору. Пример:

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct AstVersion {
    version: String,
}

fn check_ast_version(json: &str) -> Result<()> {
    let v: AstVersion = serde_json::from_str(json)?;
    let major = v.version.split('.').next().unwrap_or("");
    if major != "1" {
        return Err(anyhow!(
            "Unsupported AST schema version: {} (expected 1.x)",
            v.version
        ));
    }
    Ok(())
}
```

Дополнительно рекомендуется в CI `code-bridge` прогонять валидацию сгенерированного JSON по `docs/ast-schema.json` (draft 2020-12) с помощью крейта `jsonschema`.

### 6.4. Процедура обновления

1. При выпуске новой мажорной версии схемы (например, 2.0) `code-bridge` должен быть обновлён для поддержки новой версии. Старые версии парсера могут оставаться доступными, но требуют явного выбора.
2. При выпуске минорной версии (1.1) `code-bridge` может продолжать использовать существующий код; новые поля будут игнорироваться при десериализации, если они не обязательны.
3. Схема `docs/ast-schema.json` версии 1.0 является неизменной. Любые изменения, нарушающие обратную совместимость, требуют выпуска версии 2.0.

---

## 7. Чеклист интеграции

1. **Добавить зависимость** `c-parser` (статическая линковка) или собрать `c-parser-ffi` (динамическая).
2. **Сконфигурировать `ParserConfig`** с учётом приоритетов (CLI > Env > TOML > defaults).
3. **Зарегистрировать парсер** в реестре плагинов `code-bridge` через `create_parser(config)`.
4. **Вызывать `parse(ParseInput { ... })`**; при `Ok` использовать `result.ast`.
5. **Сериализовать** через `ast_to_json(&result.ast, source)` и передать JSON генератору.
6. **Обрабатывать `result.errors()`** отдельно для диагностики.
7. **Проверять версию JSON** (`"1.0"`) перед использованием.
8. **В CI проверять** соответствие JSON схеме `docs/ast-schema.json`.
9. **Протестировать non-UTF-8 источник**: `ast_to_json` не должен паниковать; в строках допустимы символы `�`.
10. **Протестировать `ErrorNode`**: подать заведомо битый код, убедиться в связности AST и валидности `error_index`.
11. **Проверить десериализацию `null` в опциональных полях**: убедиться, что `ast_from_json` для JSON с `"is_macro": null`, `"suffix": null`, `"encoding": null`, `"value": null` (в `Enumerator`) не вызывает ошибок в `code-bridge` и корректно трактует их как `None` (согласно API-контракту §4.5). На своём выходе `code-bridge` не должен полагаться на то, что `is_macro` всегда присутствует.
12. **Для FFI**: проверить парность `c_parser_parse` / `c_parser_free_string`, отсутствие утечек (valgrind/ASan).
13. **Для инкрементального режима**: использовать `IncrementalSession` с корректным временем жизни буфера; помнить, что JSON генерируется по запросу.

---

## 8. Ссылки

- `docs/ast-schema.json` — нормативная JSON-схема AST v1.0.
- `docs/architecture.md` — устройство арены, лексера, парсера.
- `docs/api.md` — описание публичного API, паттерн `SerializableAst`, обработка кодировок.
- `docs/parser-algorithm.md` — рекурсивный спуск + Pratt, разрешение `typedef`.
- `examples/embed_into_rust.rs` — пример статической интеграции.
- `tests/schema_validation.rs` — эталон валидации JSON (включая non-UTF-8 и `null_in_optional_fields_roundtrip`).
- ТЗ v2.2, §7 — исходные требования к API, FFI и интеграции.
- API-контракт v1.2 — детальное описание типов и контрактов.
