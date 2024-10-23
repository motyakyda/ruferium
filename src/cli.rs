#![deny(missing_docs)]

use clap::{Args, Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::Shell;
use libium::config::{
    filters::{self, Filter},
    structs::ModLoader,
};
use std::path::PathBuf;

#[derive(Parser)]
#[clap(author, version, about)]
#[clap(arg_required_else_help = true)]
pub struct Ferium {
    #[clap(subcommand)]
    pub subcommand: SubCommands,
    #[clap(long, short)]
    pub threads: Option<usize>,
    #[clap(long, short = 'p')]
    pub parallel_network: Option<usize>,
    #[clap(long, visible_alias = "gh")]
    pub github_token: Option<String>,
    #[clap(long, visible_alias = "cf")]
    pub curseforge_api_key: Option<String>,
    #[clap(long, short, visible_aliases = ["config", "conf"])]
    #[clap(value_hint(ValueHint::FilePath))]
    pub config_file: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum SubCommands {
    Add {
        #[clap(required = true)]
        identifiers: Vec<String>,
        #[clap(long, short, visible_alias = "override")]
        force: bool,
        #[command(flatten)]
        filters: FilterArguments,
    },
    Scan {
        #[clap(long, short, default_value_t)]
        platform: Platform,
        #[clap(long, short,
            visible_aliases = ["dir", "folder"],
            aliases = ["output_directory", "out_dir"]
        )]
        directory: Option<PathBuf>,
        #[clap(long, short, visible_alias = "override")]
        force: bool,
    },
    Complete {
        #[clap(value_enum)]
        shell: Shell,
    },
    #[clap(visible_alias = "mods")]
    List {
        #[clap(long, short)]
        verbose: bool,
        #[clap(long, short, visible_alias = "md")]
        markdown: bool,
    },
    Modpack {
        #[clap(subcommand)]
        subcommand: Option<ModpackSubCommands>,
    },
    Modpacks,
    Profile {
        #[clap(subcommand)]
        subcommand: Option<ProfileSubCommands>,
    },
    Profiles,
    #[clap(visible_alias = "rm")]
    Remove {
        mod_names: Vec<String>,
    },
    #[clap(visible_aliases = ["download", "install"])]
    Upgrade,
}

#[derive(Subcommand)]
pub enum ProfileSubCommands {
    #[clap(visible_aliases = ["config", "conf"])]
    Configure {
        #[clap(long, short = 'v')]
        game_versions: Vec<String>,
        #[clap(long, short = 'l')]
        #[clap(value_enum)]
        mod_loaders: Vec<ModLoader>,
        #[clap(long, short)]
        name: Option<String>,
        #[clap(long, short)]
        #[clap(value_hint(ValueHint::DirPath))]
        output_dir: Option<PathBuf>,
    },
    #[clap(visible_alias = "new")]
    Create {
        #[clap(long, short, visible_aliases = ["copy", "duplicate"])]
        #[expect(clippy::option_option)]
        import: Option<Option<String>>,
        #[clap(long, short = 'v')]
        game_version: Vec<String>,
        #[clap(long, short)]
        #[clap(value_enum)]
        mod_loader: Option<ModLoader>,
        #[clap(long, short)]
        name: Option<String>,
        #[clap(long, short)]
        #[clap(value_hint(ValueHint::DirPath))]
        output_dir: Option<PathBuf>,
    },
    #[clap(visible_aliases = ["remove", "rm"])]
    Delete {
        profile_name: Option<String>,
        #[clap(long, short)]
        switch_to: Option<String>,
    },
    Info,
    List,
    Switch {
        profile_name: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ModpackSubCommands {
    Add {
        identifier: String,
        #[clap(long, short)]
        #[clap(value_hint(ValueHint::DirPath))]
        output_dir: Option<PathBuf>,
        #[clap(long, short)]
        install_overrides: Option<bool>,
    },
    #[clap(visible_aliases = ["config", "conf"])]
    Configure {
        #[clap(long, short)]
        #[clap(value_hint(ValueHint::DirPath))]
        output_dir: Option<PathBuf>,
        #[clap(long, short)]
        install_overrides: Option<bool>,
    },
    #[clap(visible_aliases = ["remove", "rm"])]
    Delete {
        modpack_name: Option<String>,
        #[clap(long, short)]
        switch_to: Option<String>,
    },
    Info,
    List,
    Switch {
        modpack_name: Option<String>,
    },
    #[clap(visible_aliases = ["download", "install"])]
    Upgrade,
}

#[derive(Args)]
#[group(id = "loader", multiple = false)]
pub struct FilterArguments {
    #[clap(long, short = 'p')]
    pub override_profile: bool,
    #[clap(long, short = 'l', group = "loader")]
    pub mod_loader_prefer: Vec<ModLoader>,
    #[clap(long, group = "loader")]
    pub mod_loader_any: Vec<ModLoader>,
    #[clap(long, short = 'v', group = "version")]
    pub game_version_strict: Vec<String>,
    #[clap(long, group = "version")]
    pub game_version_minor: Vec<String>,
    #[clap(long, short = 'c')]
    pub release_channel: Option<filters::ReleaseChannel>,
    #[clap(long, short = 'n')]
    pub filename: Option<String>,
    #[clap(long, short = 't')]
    pub title: Option<String>,
    #[clap(long, short = 'd')]
    pub description: Option<String>,
}

impl From<FilterArguments> for Vec<Filter> {
    fn from(value: FilterArguments) -> Self {
        let mut filters = vec![];

        if !value.mod_loader_prefer.is_empty() {
            filters.push(Filter::ModLoaderPrefer(value.mod_loader_prefer));
        }
        if !value.mod_loader_any.is_empty() {
            filters.push(Filter::ModLoaderAny(value.mod_loader_any));
        }
        if !value.game_version_strict.is_empty() {
            filters.push(Filter::GameVersionStrict(value.game_version_strict));
        }
        if !value.game_version_minor.is_empty() {
            filters.push(Filter::GameVersionMinor(value.game_version_minor));
        }
        if let Some(release_channel) = value.release_channel {
            filters.push(Filter::ReleaseChannel(release_channel));
        }
        if let Some(regex) = value.filename {
            filters.push(Filter::Filename(regex));
        }
        if let Some(regex) = value.title {
            filters.push(Filter::Title(regex));
        }
        if let Some(regex) = value.description {
            filters.push(Filter::Description(regex));
        }

        filters
    }
}

#[derive(Clone, Copy, Default, ValueEnum)]
pub enum Platform {
    #[default]
    #[clap(alias = "mr")]
    Modrinth,
    #[clap(alias = "cf")]
    Curseforge,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Modrinth => write!(f, "modrinth"),
            Self::Curseforge => write!(f, "curseforge"),
        }
    }
}
