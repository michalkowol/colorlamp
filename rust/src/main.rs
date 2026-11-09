mod ble;
mod cli;
mod core;
mod mic;
mod protocol;
mod segments;
mod store;
mod tui;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = cli::Cli::parse();
    let path = args.state.clone().unwrap_or_else(store::default_path);
    let mut state = core::State::new(store::Store::load(&path), path);
    let (ble, error) = ble::Ble::new().await;
    state.ble_error = error;
    let core = core::Core::new(state, ble);
    match args.command.take() {
        None | Some(cli::Command::Tui) => tui::run(core).await,
        Some(command) => cli::run(core, args, command).await
    }
}
