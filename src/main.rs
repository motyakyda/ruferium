#![deny( 
    clippy::all,
    clippy::perf,
    clippy::cargo,
    clippy::style,
    clippy::pedantic,
    clippy::suspicious,
    clippy::complexity,
    clippy::create_dir,
    clippy::unwrap_used,
    clippy::expect_used, // используйте anyhow::Context вместо этого
    clippy::correctness,
    clippy::allow_attributes,
)]
#![warn(clippy::dbg_macro)]
#![expect(clippy::multiple_crate_versions, clippy::too_many_lines)]

mod add;
mod cli;
mod download;
mod file_picker;
mod subcommands;

use anyhow::{anyhow, bail, ensure, Result};
use clap::{CommandFactory, Parser};
use cli::{Ferium, ModpackSubCommands, ProfileSubCommands, SubCommands};
use colored::{ColoredString, Colorize};
use indicatif::ProgressStyle;
use libium::{
    config::{
        self,
        filters::ProfileParameters as _,
        structs::{Config, ModIdentifier, Modpack, Profile},
        DEFAULT_CONFIG_PATH,
    },
    iter_ext::IterExt as _,
};
use std::{
    env::{set_var, var_os},
    process::ExitCode,
    sync::{LazyLock, OnceLock},
};

const CROSS: &str = "×";
static TICK: LazyLock<ColoredString> = LazyLock::new(|| "✓".green());

pub static PARALLEL_NETWORK: OnceLock<usize> = OnceLock::new();
pub const DEFAULT_PARALLEL_NETWORK: usize = 10;

/// Темы Indicatif
#[expect(clippy::expect_used)]
pub static STYLE_NO: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::default_bar()
        .template("{spinner} {elapsed} [{wide_bar:.cyan/blue}] {pos:.cyan}/{len:.blue}")
        .expect("Ошибка разбора шаблона индикатора прогресса")
        .progress_chars("#>-")
});
#[expect(clippy::expect_used)]
pub static STYLE_BYTE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::default_bar()
        .template(
            "{spinner} {bytes_per_sec} [{wide_bar:.cyan/blue}] {bytes:.cyan}/{total_bytes:.blue}",
        )
        .expect("Ошибка разбора шаблона индикатора прогресса")
        .progress_chars("#>-")
});

fn main() -> ExitCode {
    #[cfg(windows)]
    // Включить цвета в conhost (командная строка или PowerShell)
    {
        #[expect(clippy::unwrap_used, reason = "Ошибок нет")]
        colored::control::set_virtual_terminal(true).unwrap();
    }

    let cli = Ferium::parse();

    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    builder.thread_name("ferium-worker");
    if let Some(threads) = cli.threads {
        builder.worker_threads(threads);
    }
    #[expect(clippy::expect_used)] // Обработки ошибок пока нет
    let runtime = builder.build().expect("Не удалось инициализировать среду Tokio");

    if let Err(err) = runtime.block_on(actual_main(cli)) {
        if !err.to_string().is_empty() {
            eprintln!("{}", err.to_string().red().bold());
            if err
                .to_string()
                .to_lowercase()
                .contains("ошибка подключения")
                || err
                    .to_string()
                    .to_lowercase()
                    .contains("ошибка отправки запроса")
            {
                eprintln!(
                    "{}",
                    "Проверьте, подключены ли вы к интернету"
                        .yellow()
                        .bold()
                );
            }
        }
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

async fn actual_main(mut cli_app: Ferium) -> Result<()> {
    if let SubCommands::Complete { shell } = cli_app.subcommand {
        clap_complete::generate(
            shell,
            &mut Ferium::command(),
            "ferium",
            &mut std::io::stdout(),
        );
        return Ok(());
    }
    if let SubCommands::Profiles = cli_app.subcommand {
        cli_app.subcommand = SubCommands::Profile {
            subcommand: Some(ProfileSubCommands::List),
        };
    }
    if let SubCommands::Modpacks = cli_app.subcommand {
        cli_app.subcommand = SubCommands::Modpack {
            subcommand: Some(ModpackSubCommands::List),
        };
    }

    if let Some(token) = cli_app.github_token {
        set_var("GITHUB_TOKEN", token);
    }
    if let Some(key) = cli_app.curseforge_api_key {
        set_var("CURSEFORGE_API_KEY", key);
    }
    if let Some(n) = cli_app.parallel_network {
        let _ = PARALLEL_NETWORK.set(n);
    }

    let mut config_file = config::get_file(
        &cli_app
            .config_file
            .or_else(|| var_os("FERIUM_CONFIG_FILE").map(Into::into))
            .unwrap_or(DEFAULT_CONFIG_PATH.clone()),
    )?;
    let mut config = config::deserialise(&libium::read_wrapper(&mut config_file)?)?;

    let mut did_add_fail = false;

    match cli_app.subcommand {
        SubCommands::Complete { .. } | SubCommands::Profiles | SubCommands::Modpacks => {
            unreachable!();
        }
        SubCommands::Scan {
            platform,
            directory,
            force,
        } => {
            let profile = get_active_profile(&mut config)?;

            let spinner = indicatif::ProgressBar::new_spinner().with_message("Чтение файлов");
            spinner.enable_steady_tick(std::time::Duration::from_millis(100));

            let ids = libium::scan(directory.as_ref().unwrap_or(&profile.output_dir), || {
                spinner.set_message("Запрос к серверам");
            })
            .await?;

            spinner.set_message("Добавление модов");

            let mut send_ids = Vec::new();
            for id in ids {
                use libium::config::structs::ModIdentifier;
                match id {
                    (filename, None, None) => {
                        println!("{} {}", "Неизвестный файл:".yellow(), filename.dimmed());
                    }
                    (_, Some(mr_id), None) => send_ids.push(ModIdentifier::ModrinthProject(mr_id)),
                    (_, None, Some(cf_id)) => {
                        send_ids.push(ModIdentifier::CurseForgeProject(cf_id));
                    }
                    (_, Some(mr_id), Some(cf_id)) => match platform {
                        cli::Platform::Modrinth => {
                            send_ids.push(ModIdentifier::ModrinthProject(mr_id));
                        }
                        cli::Platform::Curseforge => {
                            send_ids.push(ModIdentifier::CurseForgeProject(cf_id));
                        }
                    },
                }
            }

            let (successes, failures) =
                libium::add(profile, send_ids, !force, false, vec![]).await?;
            spinner.finish_and_clear();

            did_add_fail = add::display_successes_failures(&successes, failures);
        }
        SubCommands::Add {
            identifiers,
            force,
            filters,
        } => {
            let profile = get_active_profile(&mut config)?;
            let override_profile = filters.override_profile;
            let filters: Vec<_> = filters.into();

            if identifiers.len() > 1 && !filters.is_empty() {
                bail!("Фильтры можно настраивать только при добавлении одного мода!")
            }

            let (successes, failures) = libium::add(
                profile,
                identifiers
                    .into_iter()
                    .map(libium::add::parse_id)
                    .collect_vec(),
                !force,
                override_profile,
                filters,
            )
            .await?;

            did_add_fail = add::display_successes_failures(&successes, failures);
        }
        SubCommands::List { verbose, markdown } => {
            let profile = get_active_profile(&mut config)?;
            check_empty_profile(profile)?;

            if verbose {
                subcommands::list::verbose(profile, markdown).await?;
            } else {
                println!(
                    "{} {} на {} {}\n",
                    profile.name.bold(),
                    format!("({} модов)", profile.mods.len()).yellow(),
                    profile
                        .filters
                        .mod_loader()
                        .map(ToString::to_string)
                        .unwrap_or_default()
                        .purple(),
                    profile
                        .filters
                        .game_versions()
                        .unwrap_or(&vec![])
                        .iter()
                        .display(", ")
                        .green(),
                );
                for mod_ in &profile.mods {
                    println!(
                        "{:20}  {}",
                        match &mod_.identifier {
                            ModIdentifier::CurseForgeProject(id) =>
                                format!("{} {:8}", "CF".red(), id.to_string().dimmed()),
                            ModIdentifier::ModrinthProject(id) =>
                                format!("{} {:8}", "MR".green(), id.dimmed()),
                            ModIdentifier::GitHubRepository(_) => "GH".purple().to_string(),
                        },
                        match &mod_.identifier {
                            ModIdentifier::ModrinthProject(_)
                            | ModIdentifier::CurseForgeProject(_) => mod_.name.bold().to_string(),
                            ModIdentifier::GitHubRepository(id) =>
                                format!("{}/{}", id.0.dimmed(), id.1.bold()),
                        },
                    );
                }
            }
        }
        SubCommands::Modpack { subcommand } => {
            let mut default_flag = false;
            let subcommand = subcommand.unwrap_or_else(|| {


                default_flag = true;
                ModpackSubCommands::List
            });

            subcommands::modpacks::subcommand(subcommand, &mut config, &mut config_file).await?;

            if default_flag {
                println!("\n(Чтобы увидеть список профилей, используйте 'ferium modpack list')");
            }
        }
        SubCommands::Profile { subcommand } => {
            let mut default_flag = false;
            let subcommand = subcommand.unwrap_or_else(|| {
                default_flag = true;
                ProfileSubCommands::List
            });

            subcommands::profile::subcommand(subcommand, &mut config, &mut config_file).await?;

            if default_flag {
                println!("\n(Чтобы увидеть список профилей, используйте 'ferium profile list')");
            }
        }
        SubCommands::Download {
            directory,
            no_retries,
            parallel,
        } => {
            download::download(
                &get_active_profile(&mut config)?,
                directory.as_deref(),
                parallel,
                !no_retries,
            )
            .await?;
        }
    }

    config::serialise(&config, &mut config_file)?;

    ensure!(
        !did_add_fail,
        "{}",
        "Некоторые моды не удалось добавить!".red()
    );

    Ok(())
}

#[expect(clippy::unwrap_used)]
fn get_active_profile<'a>(config: &'a mut Config) -> Result<&'a mut Profile> {
    Ok(config
        .profiles
        .iter_mut()
        .find(|p| p.active)
        .ok_or_else(|| anyhow!("Нет активного профиля!\n(Вы можете создать профиль с помощью 'ferium profile create')"))?)
}

fn check_empty_profile(profile: &Profile) -> Result<()> {
    ensure!(
        !profile.mods.is_empty(),
        "{}",
        "Профиль не содержит модов.".yellow()
    );
    Ok(())
}
