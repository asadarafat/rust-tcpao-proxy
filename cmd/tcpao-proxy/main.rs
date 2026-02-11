use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use tcpao_proxy::config::{Config, LogFormat, Mode};
use tcpao_proxy::error::Result;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ModeArg {
    Initiator,
    Terminator,
}

impl From<ModeArg> for Mode {
    fn from(value: ModeArg) -> Self {
        match value {
            ModeArg::Initiator => Mode::Initiator,
            ModeArg::Terminator => Mode::Terminator,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LogFormatArg {
    Text,
    Json,
}

impl From<LogFormatArg> for LogFormat {
    fn from(value: LogFormatArg) -> Self {
        match value {
            LogFormatArg::Text => LogFormat::Text,
            LogFormatArg::Json => LogFormat::Json,
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "tcpao-proxy")]
#[command(about = "BMP TCP-AO sidecar proxy (PoC scaffold)")]
struct Cli {
    #[arg(long, value_enum)]
    mode: ModeArg,

    #[arg(long)]
    config: PathBuf,

    #[arg(long, value_enum)]
    log_format: Option<LogFormatArg>,

    #[arg(long)]
    dry_run: bool,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        error!(error = %err, "proxy exited with error");
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let mode: Mode = cli.mode.into();

    let config = Config::load(&cli.config)?;
    init_tracing(
        cli.log_format
            .map(Into::into)
            .unwrap_or(config.global.log_format),
    );
    config.validate(mode)?;

    info!(mode = ?mode, config = %cli.config.display(), summary = %config.redacted_summary(), "config loaded");

    if cli.dry_run {
        info!("dry-run successful");
        return Ok(());
    }

    match mode {
        Mode::Initiator => tcpao_proxy::mode_initiator::run(config).await,
        Mode::Terminator => tcpao_proxy::mode_terminator::run(config).await,
    }
}

fn init_tracing(log_format: LogFormat) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let builder = tracing_subscriber::fmt().with_env_filter(env_filter);

    match log_format {
        LogFormat::Text => {
            builder.compact().init();
        }
        LogFormat::Json => {
            builder.json().init();
        }
    }
}
