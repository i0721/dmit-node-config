mod artifacts;
mod config;
mod deploy;
mod diagnostics;
mod dns;
mod host;
mod provision;
mod render;
mod setup;
mod store;
#[cfg(test)]
mod tests;
mod upgrade;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long, default_value = "/etc/dmit-node", global = true)]
    config_dir: PathBuf,
    #[arg(long, default_value = "/var/lib/dmit-node", global = true)]
    state_dir: PathBuf,
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Aggregate environment checks without changing the host.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Show versions, services, receipts, certificates and interrupted work.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Preview or explicitly create the single source of configuration.
    Setup(setup::Setup),
    /// Preview or run a DMIT deployment, preserving existing inputs and using apply transactions.
    Deploy(deploy::Deploy),
    /// Validate schema compatibility; unsupported schemas are never rewritten.
    Migrate,
    /// Install a signed exact tool version; preview unless --execute is supplied.
    Upgrade {
        #[arg(long)]
        version: String,
        #[arg(long, default_value = "/usr/local")]
        prefix: PathBuf,
        #[arg(long)]
        execute: bool,
    },
    /// Restore entrypoints and service states captured before first apply.
    Uninstall {
        #[arg(long)]
        execute: bool,
    },
    /// Create missing configuration and credentials; never reset existing values.
    Init {
        #[arg(long)]
        example: Option<PathBuf>,
        /// Explicitly rotate one or more credential/path fields.
        #[arg(long, value_delimiter = ',')]
        rotate: Vec<String>,
    },
    Validate,
    Render,
    Diff {
        release: String,
    },
    /// Check candidate with installed tools and pin the reference client.
    Check {
        release: String,
        #[arg(long)]
        client: PathBuf,
        #[arg(long)]
        client_sha256: String,
        #[arg(long)]
        client_version: String,
        #[arg(long)]
        probe_url: String,
    },
    Apply {
        release: String,
    },
    Rollback,
    /// Restore an interrupted deployment from its protected transaction journal.
    Recover,
    ExportClients {
        #[arg(long)]
        output: PathBuf,
    },
    /// Explicitly back up existing unmanaged entrypoints for a later apply.
    Takeover {
        release: String,
    },
    /// Revalidate certificates and reload Nginx after certificate renewal.
    RenewHook,
    /// Print JSON Schema for node.toml (represented as a JSON object).
    Schema,
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if matches!(cli.command, Action::Schema) {
        println!(
            "{}",
            serde_json::to_string_pretty(&schemars::schema_for!(config::Config))?
        );
        return Ok(());
    }
    let store = store::Store::new(cli.config_dir, cli.state_dir)?;
    match cli.command {
        Action::Doctor { json } => diagnostics::doctor(&store, json)?,
        Action::Status { json } => diagnostics::status(&store, json)?,
        Action::Setup(setup) => setup::run(&store, setup)?,
        Action::Deploy(deploy) => deploy::run(&store, deploy)?,
        Action::Migrate => {
            let _guard = store.lock()?;
            diagnostics::migrate(&store)?;
        }
        Action::Upgrade {
            version,
            prefix,
            execute,
        } => upgrade::run(&store, &version, &prefix, execute)?,
        Action::Uninstall { execute } => {
            let _guard = if execute { Some(store.lock()?) } else { None };
            host::uninstall(&store, execute)?;
        }
        Action::Init { example, rotate } => {
            let _guard = store.lock()?;
            config::init(&store.config_dir, example.as_deref(), &rotate)?;
            println!("Initialized; edit node.toml, then validate and render.");
        }
        Action::Validate => {
            store.inputs()?;
            println!("Configuration valid.");
        }
        Action::Render => {
            let _guard = store.lock()?;
            println!("{}", store.render()?);
        }
        Action::Diff { release } => print!("{}", store.diff(&release)?),
        Action::Check {
            release,
            client,
            client_sha256,
            client_version,
            probe_url,
        } => {
            let _guard = store.lock()?;
            host::check(
                &store,
                &release,
                host::Reference {
                    binary: client,
                    sha256: client_sha256,
                    version: client_version,
                    probe_url,
                },
            )?;
            println!("Preflight passed; live handshakes will be required during apply.");
        }
        Action::Apply { release } => {
            let _guard = store.lock()?;
            host::apply(&store, &release)?;
            println!("Applied {release}; services and all three handshakes passed.");
        }
        Action::Rollback => {
            let _guard = store.lock()?;
            let release = store
                .pointer("previous")?
                .ok_or_else(|| anyhow::anyhow!("No previous successful release"))?;
            host::apply(&store, &release)?;
            println!("Rolled back to {release}.");
        }
        Action::ExportClients { output } => {
            let _guard = store.lock()?;
            store.export(&output)?;
            println!("Exported current successful client profiles.");
        }
        Action::Recover => {
            let _guard = store.recovery_lock()?;
            let _lease = provision::Lease::hold(&_guard);
            if store.state_dir.join("deployment.json").exists() {
                deploy::recover(&store)?;
            } else {
                host::recover(&store)?;
            }
            println!("Interrupted deployment restored and verified.");
        }
        Action::Takeover { release } => {
            let _guard = store.lock()?;
            host::takeover(&store, &release)?;
            println!("Backups and takeover diff saved; entrypoints remain unchanged until apply.");
        }
        Action::RenewHook => {
            let _guard = store.lock()?;
            host::renew(&store)?;
            println!("Certificates checked and Nginx reloaded.");
        }
        Action::Schema => unreachable!(),
    }
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("nodectl: {err:#}");
        std::process::exit(1);
    }
}
