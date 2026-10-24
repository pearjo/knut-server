//! The `knut` command line. Parses the arguments and runs the matching
//! command from the library.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Knut, a small smart-home daemon for Apple Home")]
struct Cli {
    /// The Ruby config.
    #[arg(short, long, global = true, env = "KNUT_CONFIG", default_value = "/etc/knut/config.rb")]
    config: PathBuf,

    /// Where pairing keys and auto/override state are kept.
    #[arg(long, global = true, env = "KNUT_STATE_DIR", default_value = "/var/lib/knut")]
    state_dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse and validate a config, then print the model.
    Check {
        /// Defaults to --config.
        config: Option<PathBuf>,
    },
    /// List devices not referenced in the config, as ready-to-paste DSL.
    Discover,
    /// Control a light or group, e.g. `knut set Kitchen/Spots 40% 2700K`.
    Set {
        /// "Room/Entity".
        entity: String,
        /// on, off, a brightness (40 or 40%), a colour temperature (2700K),
        /// transition=<seconds>.
        #[arg(required = true, num_args = 1..)]
        values: Vec<String>,
    },
    /// Run the daemon.
    Run,
    /// Forget all HomeKit pairings, so the bridge can be added to Apple Home
    /// again. Run it while the daemon is stopped.
    Unpair,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,hap=warn,libmdns=error")).init();

    let runtime = tokio::runtime::Runtime::new().expect("cannot start the async runtime");
    let result = match cli.command {
        Command::Check { config } => knut::check(config.as_deref().unwrap_or(&cli.config)),
        Command::Discover => runtime.block_on(knut::discover(&cli.config, &cli.state_dir)),
        Command::Set { entity, values } => runtime.block_on(knut::set(&cli.config, &cli.state_dir, &entity, &values)),
        Command::Run => runtime.block_on(knut::run(&cli.config, &cli.state_dir)),
        Command::Unpair => knut::unpair(&cli.state_dir),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}
