//! `rideshell history`: bring Claude Code / Codex conversations into accounts.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use rideshell_core::history;
use rideshell_core::providers::{self, AccountDirProvider, ProviderContext};
use rideshell_core::store::{Profile, Store};

use super::Ctx;
use crate::cli::HistoryCommand;
use crate::ui;

fn provider(tool: &str) -> Result<&'static AccountDirProvider> {
    match tool {
        "claude" => Ok(&providers::CLAUDE),
        "codex" => Ok(&providers::CODEX),
        other => bail!("conversation history is supported for claude and codex, not '{other}'"),
    }
}

/// The account's current folder and the tool's original one.
fn folders(ctx: &Ctx, store: &Store, p: &Profile, tool: &str) -> Result<(PathBuf, Option<PathBuf>)> {
    let prov = provider(tool)?;
    let settings = store.identities(p.id)?.remove(tool).unwrap_or_default();
    let accounts = ctx.paths.accounts_dir();
    let dir = prov.config_dir(&settings, &ProviderContext { profile: &p.name, accounts_dir: &accounts });
    Ok((dir, providers::default_account_dir(tool)))
}

fn same(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (dunce::canonicalize(a), dunce::canonicalize(b)) {
        (Ok(a), Ok(b)) => a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()),
        _ => false,
    }
}

pub fn run(ctx: &Ctx, cmd: HistoryCommand) -> Result<i32> {
    match cmd {
        HistoryCommand::Status => status(ctx),
        HistoryCommand::Adopt { tool, profile, yes } => adopt(ctx, &tool, &profile, yes),
        HistoryCommand::Share { tool, profile } => share(ctx, &tool, &profile),
        HistoryCommand::Separate { tool, profile } => separate(ctx, &tool, &profile),
    }
}

fn status(ctx: &Ctx) -> Result<i32> {
    let store = ctx.store()?;
    let mut rows = vec![vec![
        ui::dim("PROFILE"),
        ui::dim("TOOL"),
        ui::dim("HISTORY"),
        ui::dim("CONVERSATIONS"),
        ui::dim("FOLDER"),
    ]];
    for p in store.list_profiles()? {
        let ids = store.identities(p.id)?;
        for tool in ["claude", "codex"] {
            if !ids.contains_key(tool) {
                continue;
            }
            let spec = history::spec(tool).expect("known tool");
            let (dir, orig) = folders(ctx, &store, &p, tool)?;
            let mode = match &orig {
                Some(o) if same(&dir, o) => ui::paint("original folder", ui::OK),
                _ if history::is_shared(&spec, &dir) => ui::paint("shared with original", ui::OK),
                _ => ui::dim("own"),
            };
            rows.push(vec![
                p.name.clone(),
                tool.to_string(),
                mode,
                history::conversation_count(&spec, &dir).to_string(),
                ui::dim(dir.display().to_string()),
            ]);
        }
    }
    ui::table(&rows);
    for tool in ["claude", "codex"] {
        if let Some(o) = providers::default_account_dir(tool) {
            let n = history::conversation_count(&history::spec(tool).expect("known"), &o);
            ui::hint(format!("{tool} original folder {} has {n} conversation(s)", o.display()));
        }
    }
    ui::hint("adopt: use the original folder (login + history) · share: own login, same history");
    Ok(0)
}

fn adopt(ctx: &Ctx, tool: &str, profile: &str, yes: bool) -> Result<i32> {
    let mut store = ctx.store()?;
    let p = store.require_profile(profile)?;
    let (dir, orig) = folders(ctx, &store, &p, tool)?;
    let orig = orig.with_context(|| format!("there is no original {tool} folder on this PC"))?;
    if same(&dir, &orig) {
        ui::success(format!("{} already uses {}", ui::accent(&p.name), orig.display()));
        return Ok(0);
    }
    anstream::println!("{} will use {}: its sign-in and all conversations there.", ui::accent(&p.name), orig.display());
    anstream::println!("Conversations from {} are moved there first (nothing is overwritten).", dir.display());
    if !yes && !ui::confirm("Continue?", true) {
        anstream::println!("Nothing changed.");
        return Ok(1);
    }
    let spec = history::spec(tool).expect("known tool");
    if history::is_shared(&spec, &dir) {
        history::unshare(&spec, &dir)?;
    }
    let moved = history::merge_into(&spec, &dir, &orig)?;
    let mut settings = store.identities(p.id)?.remove(tool).unwrap_or_default();
    settings.remove("account");
    settings.insert("config_dir".into(), orig.to_string_lossy().into_owned());
    store.set_identity(&p.name, tool, &settings)?;
    ui::success(format!(
        "{} now uses {} ({} item(s) moved{})",
        ui::accent(&p.name),
        orig.display(),
        moved.files,
        if moved.skipped > 0 { format!(", {} already there", moved.skipped) } else { String::new() }
    ));
    ui::hint(format!("its previous folder {} is kept (its sign-in is no longer used)", dir.display()));
    Ok(0)
}

fn share(ctx: &Ctx, tool: &str, profile: &str) -> Result<i32> {
    let store = ctx.store()?;
    let p = store.require_profile(profile)?;
    if !store.identities(p.id)?.contains_key(tool) {
        bail!("{} has no {tool} account yet (`rideshell identity set {} {tool}`)", p.name, p.name);
    }
    let (dir, orig) = folders(ctx, &store, &p, tool)?;
    let orig = orig.with_context(|| format!("there is no original {tool} folder on this PC"))?;
    if same(&dir, &orig) {
        ui::success(format!("{} already uses {} (history included)", ui::accent(&p.name), orig.display()));
        return Ok(0);
    }
    let spec = history::spec(tool).expect("known tool");
    let moved = history::share(&spec, &dir, &orig)?;
    ui::success(format!(
        "{} keeps its own {tool} sign-in and now shares conversation history with {}",
        ui::accent(&p.name),
        orig.display()
    ));
    if moved.files > 0 {
        ui::hint(format!("{} of its own conversation item(s) were added to the shared history", moved.files));
    }
    Ok(0)
}

fn separate(ctx: &Ctx, tool: &str, profile: &str) -> Result<i32> {
    let store = ctx.store()?;
    let p = store.require_profile(profile)?;
    let (dir, _) = folders(ctx, &store, &p, tool)?;
    let spec = history::spec(tool).expect("known tool");
    if !history::is_shared(&spec, &dir) {
        ui::success(format!("{}'s {tool} history is already its own", ui::accent(&p.name)));
        return Ok(0);
    }
    history::unshare(&spec, &dir)?;
    ui::success(format!(
        "{} has its own {tool} history again (shared conversations stay where they are)",
        ui::accent(&p.name)
    ));
    Ok(0)
}
