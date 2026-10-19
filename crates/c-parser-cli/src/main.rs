//! `c-parser-cli` — standalone CLI wrapper for the `c-parser` library.
//!
//! ## Usage
//!
//! ```text
//! c-parser-cli parse <input.c> [OPTIONS]
//! c-parser-cli parse-incremental <input.c> --start <byte> --end <byte> [OPTIONS]
//! ```
//!
//! ## Exit codes
//!
//! | Code | Meaning                                |
//! |------|----------------------------------------|
//! | 0    | Success (even with non-fatal errors)   |
//! | 1    | Fatal error (I/O, parse failure, etc.) |
//! | 2    | Invalid arguments                      |
//!
//! ## Configuration priority (highest to lowest)
//!
//! 1. CLI arguments
//! 2. Environment variables (`C_PARSER_LOG`, `C_PARSER_CONFIG`)
//! 3. TOML configuration file (`--config` or `C_PARSER_CONFIG`)
//! 4. Built-in defaults

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;

use anyhow::Context;
use c_parser::{
    ast_to_json, ast_to_pretty_json, create_parser, CDialect, IncrementalSession, ParseInput,
    ParserConfig, PreprocessingMode, Span,
};
use clap::{error::ErrorKind, Parser, Subcommand, ValueEnum};
use tracing::{debug, error, info};

// =============================================================================
// CLI definition (clap derive)
// =============================================================================

/// Standalone C parser — parse C source files and output a JSON AST.
#[derive(Parser)]
#[command(
    name = "c-parser-cli",
    version = env!("CARGO_PKG_VERSION"),
    about = "Parse C source files and output a JSON AST.\n\nConfiguration priority: CLI args > env vars > TOML file > defaults.",
    after_help = "ENVIRONMENT:\n  C_PARSER_LOG     Log level (trace, debug, info, warn, error)\n  C_PARSER_CONFIG  Path to default TOML configuration file"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to save JSON output (default: stdout).
    #[arg(short = 'o', long = "output", global = true, value_name = "FILE")]
    output: Option<PathBuf>,

    /// C dialect: c89, c99, c11, c17 [default: c11].
    #[arg(long = "dialect", value_enum, global = true, value_name = "DIALECT")]
    dialect: Option<DialectArg>,

    /// Preprocessing mode: preprocessed, single-tu [default: preprocessed].
    #[arg(long = "preprocessing", value_enum, global = true, value_name = "MODE")]
    preprocessing: Option<PreprocessingArg>,

    /// Include comments in the token stream.
    #[arg(long = "include-comments", global = true, default_value_t = false)]
    include_comments: bool,

    /// Format JSON with indentation (pretty-print).
    #[arg(long = "pretty", global = true, default_value_t = false)]
    pretty: bool,

    /// Log level: trace, debug, info, warn, error [default: info].
    #[arg(long = "log-level", global = true, value_name = "LEVEL")]
    log_level: Option<String>,

    /// Path to TOML configuration file.
    #[arg(long = "config", global = true, value_name = "FILE")]
    config: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Full parse of a C source file — output JSON AST.
    Parse {
        /// Input C source file.
        input: PathBuf,
    },
    /// Parse a file and apply an incremental edit.
    ///
    /// The file is parsed in full first; then an edit described by
    /// --start/--end is applied and the updated JSON AST is emitted.
    ParseIncremental {
        /// Input C source file.
        input: PathBuf,

        /// Start byte offset of the changed range.
        #[arg(long = "start", value_name = "BYTE")]
        start: usize,

        /// End byte offset of the changed range.
        #[arg(long = "end", value_name = "BYTE")]
        end: usize,
    },
}

// ---------------------------------------------------------------------------
// CLI argument ↔ library type conversions
// ---------------------------------------------------------------------------

#[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
enum DialectArg {
    C89,
    C99,
    C11,
    C17,
}

impl From<DialectArg> for CDialect {
    fn from(arg: DialectArg) -> Self {
        match arg {
            DialectArg::C89 => CDialect::C89,
            DialectArg::C99 => CDialect::C99,
            DialectArg::C11 => CDialect::C11,
            DialectArg::C17 => CDialect::C17,
        }
    }
}

impl std::fmt::Display for DialectArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DialectArg::C89 => write!(f, "c89"),
            DialectArg::C99 => write!(f, "c99"),
            DialectArg::C11 => write!(f, "c11"),
            DialectArg::C17 => write!(f, "c17"),
        }
    }
}

#[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
enum PreprocessingArg {
    /// Input is already preprocessed (recommended).
    Preprocessed,
    /// Raw translation unit: #- directives are stored as AST nodes.
    #[value(name = "single-tu")]
    SingleTu,
}

impl From<PreprocessingArg> for PreprocessingMode {
    fn from(arg: PreprocessingArg) -> Self {
        match arg {
            PreprocessingArg::Preprocessed => PreprocessingMode::Preprocessed,
            PreprocessingArg::SingleTu => PreprocessingMode::SingleTu,
        }
    }
}

impl std::fmt::Display for PreprocessingArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreprocessingArg::Preprocessed => write!(f, "preprocessed"),
            PreprocessingArg::SingleTu => write!(f, "single-tu"),
        }
    }
}

// =============================================================================
// TOML configuration
// =============================================================================

/// Top-level TOML structure.
///
/// # Example
/// ```toml
/// [parser]
/// dialect = "c11"
/// preprocessing = "preprocessed"
/// include_comments = false
/// ```
#[derive(serde::Deserialize, Default)]
struct TomlConfig {
    parser: Option<TomlParser>,
}

#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct TomlParser {
    dialect: Option<String>,
    log_level: Option<String>,
    preprocessing: Option<String>,
    include_comments: Option<bool>,
}

// =============================================================================
// Resolved effective configuration
// =============================================================================

/// The fully resolved configuration after applying all layers.
struct EffectiveConfig {
    dialect: CDialect,
    preprocessing: PreprocessingMode,
    include_comments: bool,
    pretty: bool,
    output: Option<PathBuf>,
}

// =============================================================================
// main
// =============================================================================

fn main() {
    // Parse CLI. Exit code 2 on invalid arguments; 0 for --help/--version.
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            let code = match e.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
                _ => 2,
            };
            process::exit(code);
        }
    };

    // ---- Resolve config file path ----
    // Priority: --config > C_PARSER_CONFIG env var > none
    let config_path = resolve_config_path(cli.config.as_deref());

    // ---- Load TOML ----
    let toml_config = config_path.as_deref().and_then(load_toml_config);

    // ---- Resolve log level (needed early for tracing init) ----
    // Priority: --log-level > C_PARSER_LOG > TOML > default ("info")
    let effective_log_level = resolve_log_level(
        cli.log_level.as_deref(),
        toml_config
            .as_ref()
            .and_then(|c| c.parser.as_ref()?.log_level.as_deref()),
    );

    // ---- Init tracing ----
    init_tracing(&effective_log_level);
    debug!("log level set to: {}", effective_log_level);
    debug!("config file: {:?}", config_path);

    // ---- Resolve final configuration ----
    let effective = EffectiveConfig {
        dialect: resolve_dialect(cli.dialect, toml_config.as_ref()),
        preprocessing: resolve_preprocessing(cli.preprocessing, toml_config.as_ref()),
        include_comments: resolve_include_comments(cli.include_comments, toml_config.as_ref()),
        pretty: cli.pretty,
        output: cli.output,
    };

    info!("c-parser-cli v{}", env!("CARGO_PKG_VERSION"));
    debug!(
        "config: dialect={:?}, preprocessing={:?}, include_comments={}, pretty={}",
        effective.dialect, effective.preprocessing, effective.include_comments, effective.pretty,
    );

    // ---- Run command ----
    let result = match &cli.command {
        Commands::Parse { input } => run_parse(input, &effective),
        Commands::ParseIncremental { input, start, end } => {
            run_parse_incremental(input, *start, *end, &effective)
        }
    };

    match result {
        Ok(()) => {
            debug!("completed successfully");
            process::exit(0);
        }
        Err(e) => {
            error!("fatal error: {:#}", e);
            eprintln!("Fatal error: {:#}", e);
            process::exit(1);
        }
    }
}

// =============================================================================
// Configuration resolution (Priority: CLI > env > TOML > default)
// =============================================================================

/// Resolve the path to the TOML config file.
///
/// Priority: CLI `--config` > `C_PARSER_CONFIG` env var > none.
fn resolve_config_path(cli_config: Option<&Path>) -> Option<PathBuf> {
    cli_config
        .map(Path::to_path_buf)
        .or_else(|| std::env::var("C_PARSER_CONFIG").ok().map(PathBuf::from))
}

/// Load a TOML config file, logging any errors.
fn load_toml_config(path: &Path) -> Option<TomlConfig> {
    match fs::read_to_string(path) {
        Ok(contents) => match toml::from_str::<TomlConfig>(&contents) {
            Ok(cfg) => {
                debug!("loaded config from {}", path.display());
                Some(cfg)
            }
            Err(e) => {
                error!("failed to parse config {}: {}", path.display(), e);
                None
            }
        },
        Err(e) => {
            error!("failed to read config {}: {}", path.display(), e);
            None
        }
    }
}

/// Resolve log level across all layers.
///
/// Priority: CLI --log-level > C_PARSER_LOG env > TOML parser.log_level > "info".
fn resolve_log_level(cli_log: Option<&str>, toml_log: Option<&str>) -> String {
    cli_log
        .map(String::from)
        .or_else(|| std::env::var("C_PARSER_LOG").ok())
        .or_else(|| toml_log.map(String::from))
        .unwrap_or_else(|| "info".to_string())
}

/// Initialize `tracing-subscriber` with the given log level.
fn init_tracing(level: &str) {
    let filter =
        tracing_subscriber::EnvFilter::try_new(format!("c_parser={level},c_parser_cli={level}"))
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_target(false)
        .with_env_filter(filter)
        .init();
}

// ---------------------------------------------------------------------------
// Per-setting resolution helpers
// ---------------------------------------------------------------------------

/// Resolve dialect.
///
/// Priority: CLI (if Some) > TOML (if valid) > default C11.
fn resolve_dialect(cli: Option<DialectArg>, toml: Option<&TomlConfig>) -> CDialect {
    if let Some(arg) = cli {
        return arg.into();
    }
    match toml
        .and_then(|t| t.parser.as_ref())
        .and_then(|p| p.dialect.as_deref())
    {
        Some("c89") => CDialect::C89,
        Some("c99") => CDialect::C99,
        Some("c11") => CDialect::C11,
        Some("c17") => CDialect::C17,
        Some(other) => {
            error!("unknown dialect in TOML: '{}', using default", other);
            CDialect::default()
        }
        None => CDialect::default(),
    }
}

/// Resolve preprocessing mode.
///
/// Priority: CLI (if Some) > TOML (if valid) > default Preprocessed.
fn resolve_preprocessing(
    cli: Option<PreprocessingArg>,
    toml: Option<&TomlConfig>,
) -> PreprocessingMode {
    if let Some(arg) = cli {
        return arg.into();
    }
    match toml
        .and_then(|t| t.parser.as_ref())
        .and_then(|p| p.preprocessing.as_deref())
    {
        Some("preprocessed") => PreprocessingMode::Preprocessed,
        Some("single-tu") => PreprocessingMode::SingleTu,
        Some(other) => {
            error!(
                "unknown preprocessing mode in TOML: '{}', using default",
                other
            );
            PreprocessingMode::default()
        }
        None => PreprocessingMode::default(),
    }
}

/// Resolve include_comments.
///
/// Priority: CLI (flag present) > TOML (if set) > default false.
///
/// Since clap fills in `false` by default, we check whether the flag was
/// explicitly set OR the value is true.  If the user passed the flag, it's
/// true.  If the user did not pass it, we check TOML, then fall back to
/// false.
///
/// NOTE: clap derive does not expose "was this argument provided" directly
/// for bool flags with default_value_t.  We work around this by treating
/// `cli` as truthy: if CLI true, always use it; if CLI false, check TOML.
fn resolve_include_comments(cli: bool, toml: Option<&TomlConfig>) -> bool {
    if cli {
        return true;
    }
    toml.and_then(|t| t.parser.as_ref())
        .and_then(|p| p.include_comments)
        .unwrap_or(false)
}

// =============================================================================
// Command implementations
// =============================================================================

/// Build a `ParserConfig` from the effective configuration.
fn make_parser_config(effective: &EffectiveConfig) -> ParserConfig {
    ParserConfig {
        dialect: effective.dialect,
        preprocessing: effective.preprocessing,
        include_comments: effective.include_comments,
        incremental: None,
    }
}

/// Run the `parse` command.
fn run_parse(input_path: &Path, effective: &EffectiveConfig) -> anyhow::Result<()> {
    let source = fs::read(input_path)
        .with_context(|| format!("failed to read input file: {}", input_path.display()))?;

    let config = make_parser_config(effective);
    let parser = create_parser(config.clone());

    info!("parsing {}", input_path.display());
    let result = parser
        .parse(ParseInput {
            source: &source,
            filename: Some(input_path.to_str().unwrap_or("<input>")),
            config,
        })
        .map_err(|e| anyhow::anyhow!("{}: {}", e.code, e.message))?;

    // Print non-fatal diagnostics to stderr.
    print_diagnostics(input_path, result.errors());

    // Serialize to JSON.
    let json = if effective.pretty {
        ast_to_pretty_json(&result.ast, &source)?
    } else {
        ast_to_json(&result.ast, &source)?
    };

    output_json(&json, effective.output.as_deref())?;
    Ok(())
}

/// Run the `parse-incremental` command.
fn run_parse_incremental(
    input_path: &Path,
    start: usize,
    end: usize,
    effective: &EffectiveConfig,
) -> anyhow::Result<()> {
    let source = fs::read(input_path)
        .with_context(|| format!("failed to read input file: {}", input_path.display()))?;

    let config = make_parser_config(effective);

    info!(
        "parsing {} incrementally (change: {}..{})",
        input_path.display(),
        start,
        end,
    );

    // Validate change range.
    if start > source.len() || end > source.len() || start > end {
        return Err(anyhow::anyhow!(
            "invalid change range: start={start}, end={end}, source length={len}",
            len = source.len(),
        ));
    }

    // Create an incremental session from the file.
    let mut session = IncrementalSession::new(ParseInput {
        source: &source,
        filename: Some(input_path.to_str().unwrap_or("<input>")),
        config,
    })
    .map_err(|e| anyhow::anyhow!("{}: {}", e.code, e.message))?;

    // Print non-fatal diagnostics from the initial parse.
    print_diagnostics(input_path, session.errors());

    // Apply the edit: the "new text" is the content in the change range
    // of the already-parsed source.
    let new_text = &source[start..end];
    debug!(
        "applying edit: {}..{} → {} bytes of new text",
        start,
        end,
        new_text.len()
    );

    session
        .apply_edit(
            Span {
                start: start as u32,
                end: end as u32,
            },
            new_text,
        )
        .map_err(|e| anyhow::anyhow!("{}: {}", e.code, e.message))?;

    // Serialize to JSON using the session's source (which may now be owned).
    let json = if effective.pretty {
        ast_to_pretty_json(session.ast(), session.source())?
    } else {
        ast_to_json(session.ast(), session.source())?
    };

    output_json(&json, effective.output.as_deref())?;
    Ok(())
}

/// Write JSON to the given file path, or to stdout if `path` is `None`.
fn output_json(json: &str, path: Option<&Path>) -> anyhow::Result<()> {
    match path {
        Some(p) => {
            fs::write(p, json)
                .with_context(|| format!("failed to write output file: {}", p.display()))?;
            info!("wrote JSON to {}", p.display());
        }
        None => {
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            handle
                .write_all(json.as_bytes())
                .with_context(|| "failed to write to stdout")?;
            // Trailing newline for terminal friendliness.
            handle
                .write_all(b"\n")
                .with_context(|| "failed to write newline to stdout")?;
        }
    }
    Ok(())
}

/// Print a list of non-fatal diagnostics to stderr.
fn print_diagnostics(_filename: &Path, errors: &[c_parser::ParseError]) {
    for err in errors {
        let severity = match err.severity {
            c_parser::Severity::Error => "error",
            c_parser::Severity::Warning => "warning",
            c_parser::Severity::Note => "note",
            c_parser::Severity::Info => "info",
        };
        eprintln!(
            "{}: {}: {} [{}.{}]",
            severity, err.code, err.message, err.span.start, err.span.end,
        );
    }
}
