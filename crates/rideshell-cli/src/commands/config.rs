use anyhow::Result;

use super::Ctx;
use crate::cli::ConfigCommand;

pub fn run(ctx: &Ctx, cmd: ConfigCommand) -> Result<i32> {
    match cmd {
        ConfigCommand::Show => {
            let config = ctx.config()?;
            anstream::println!("# effective configuration ({})", ctx.paths.config_file().display());
            anstream::print!("{}", config.to_toml());
        }
        ConfigCommand::Path => {
            anstream::println!("config    {}", ctx.paths.config_file().display());
            anstream::println!("database  {}", ctx.paths.db_file().display());
        }
    }
    Ok(0)
}
