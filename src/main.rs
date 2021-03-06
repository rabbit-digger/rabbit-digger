use std::path::PathBuf;

use anyhow::Result;
use rabbit_digger::{config::Config, controller};
use structopt::StructOpt;
use tokio::fs::read_to_string;

#[derive(StructOpt)]
struct Args {
    /// Path to config file
    #[structopt(
        short,
        long,
        env = "RD_CONFIG",
        parse(from_os_str),
        default_value = "config.yaml"
    )]
    config: PathBuf,
}

async fn real_main(args: Args) -> Result<()> {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "rabbit_digger=trace")
    }
    tracing_subscriber::fmt::init();

    let content = read_to_string(args.config).await?;
    let config: Config = serde_yaml::from_str(&content)?;

    let controller = controller::Controller::new();

    controller.run(config).await?;

    Ok(())
}

#[paw::main]
#[tokio::main]
async fn main(args: Args) -> Result<()> {
    match real_main(args).await {
        Ok(()) => {}
        Err(e) => tracing::error!("Process exit: {:?}", e),
    }
    Ok(())
}
