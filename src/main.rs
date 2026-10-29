mod actions;
mod alt_tui;
mod conf_api;

use anyhow::{Context, Result};
use serde::Deserialize;

#[cfg(target_family = "unix")]
use serde::{Deserializer, de::Error};

use std::fs::File;
use std::process::ExitCode;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

use clap::Parser;

use crate::conf_api::HasAttr;

// Command line interface for clap
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    /// Opens the terminal interface
    View,
    /// Opens a page to be edited, using either the id or getting the last page
    Edit {
        #[command(flatten)]
        target: PageTarget,
    },
    /// Print n bytes of the content as a preview of the page rather than editing
    Preview {
        #[command(flatten)]
        target: PageTarget,
        /// Number of bytes to print
        #[arg(short, long, default_value_t = 1000)]
        bytes: usize,
    },
    /// Create a new page, either from an existing file or from scratch
    New {
        /// Use an existing file to create the new page
        #[arg(long, short)]
        path: Option<String>,
        /// The title of the new page
        #[arg(long, short)]
        title: String,
        /// Open the page for editing after it has been created
        #[arg(long, short)]
        edit: bool,
    },
    /// Deletes a page given its ID
    Delete {
        /// ID of the page to delete
        id: String,
    },
    /// Lists spaces, pages within a space, or pages with the same title across spaces
    List {
        #[command(subcommand)]
        list: ListCmd,
    },
    /// Convert stdin input md <-> html
    Convert {
        #[arg(long)]
        from: Format,
    },
    /// Deletes all local copies of pages
    Purge,
}

#[derive(Debug, clap::Subcommand)]
enum ListCmd {
    Spaces,
    Pages {
        #[arg(short, long)]
        title: Option<String>,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum Format {
    /// Convert stdin input from md to html
    Md,
    /// Convert stdin input from html to md
    Html,
}

#[derive(Debug, clap::Args)]
#[group(required = true, multiple = false)]
struct PageTarget {
    #[arg(long)]
    last: bool,
    #[arg(short, long)]
    id: Option<String>,
}

impl From<PageTarget> for PageRef {
    fn from(value: PageTarget) -> Self {
        value.id.map_or(PageRef::Last, PageRef::Id)
    }
}

enum PageRef {
    Last,
    Id(String),
}

// Config structure. Note deserialize_with for save_location, see fn
// Deserialisation for history location requires a different function to deal with the optional
// case
#[cfg(target_family = "unix")]
#[derive(Deserialize, Debug, Clone)]
struct Config {
    #[serde(deserialize_with = "from_tilde_path")]
    save_location: PathBuf,
    #[serde(default, deserialize_with = "from_tilde_path_optional")]
    history_location: Option<PathBuf>,
    auto_sync: Option<bool>,
    api: Api,
    editor: Option<Editor>,
}

#[cfg(target_family = "windows")]
#[derive(Deserialize, Debug, Clone)]
struct Config {
    save_location: PathBuf,
    history_location: Option<PathBuf>,
    auto_sync: Option<bool>,
    api: Api,
    editor: Option<Editor>,
}

impl Config {
    fn read_config<P: AsRef<Path>>(file_name: &P) -> Result<Config> {
        let mut contents = String::new();
        let mut file = File::open(file_name).context("Config file could not be found")?;
        file.read_to_string(&mut contents)
            .context("File is not readable")?;
        toml::from_str::<Config>(contents.as_str())
            .context("The config file could not be parsed: check the formatting")
    }
}

#[derive(Deserialize, Debug, Clone)]
struct Api {
    confluence_domain: String,
    username: String,
    token: String,
    label: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct Editor {
    editor: String,
    args: Option<Vec<String>>,
}

// Implements a custom deserializer for save_location that automatically
// expands the tilde to the users home directory
#[cfg(target_family = "unix")]
fn from_tilde_path<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    expanduser::expanduser(s).map_err(D::Error::custom)
}

// Same as the above deserialiser but handles the optional case for history_location
#[cfg(target_family = "unix")]
fn from_tilde_path_optional<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Deserialize::deserialize(deserializer)?;
    if let Some(s) = s {
        return Ok(Some(expanduser::expanduser(s).map_err(D::Error::custom)?));
    }

    Ok(None)
}

fn main() -> std::process::ExitCode {
    let cli = Args::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Args) -> Result<()> {
    let config = get_config()?;

    match cli.action {
        Action::Edit { target } => actions::edit(&config, &target.into()),
        Action::View => alt_tui::display(&config),
        Action::New { path, title, edit } => actions::cli_new_page(
            &config,
            &edit,
            title.clone(),
            path.map(get_path).transpose()?.as_deref(),
        )
        .inspect(|_| println!("Page {} created successfully", title)),
        Action::Delete { id } => actions::delete_page_by_id(&config.api, &id)
            .inspect(|_| println!("Page deleted successfully")),
        Action::List { list } => match list {
            ListCmd::Spaces => {
                render_name_id_list(&actions::load_space_list(&config.api)?);
                Ok(())
            }
            ListCmd::Pages { title } => match title {
                Some(title) => actions::list_page_by_title(&config.api, &title),
                None => {
                    render_name_id_list(&actions::load_page_list_select_space(&config.api)?);
                    Ok(())
                }
            },
        },
        Action::Preview { target, bytes } => {
            actions::render_page_preview(&config, &target.into(), bytes)
        }
        Action::Convert { from } => match from {
            Format::Md => {
                println!("{}", actions::convert_md_string_html()?);
                Ok(())
            }
            Format::Html => {
                println!("{}", actions::convert_html_string_md()?);
                Ok(())
            }
        },
        Action::Purge => actions::delete_local_files(&config)
            .inspect(|_| println!("Local pages purged successfully")),
    }
}

// Helper function to add the home dir to the config path. Config is expected to be in EITHER
// $XDG_CONFIG_HOME/concmd/config.toml OR $HOME/.config/concmd/config.toml (these are usually the
// same)
#[cfg(target_family = "unix")]
fn get_config() -> Result<Config> {
    let config_location = match std::env::var("XDG_CONFIG_HOME") {
        Ok(xdg_config_string) => PathBuf::from_iter([&xdg_config_string, "concmd/config.toml"]),
        Err(_) => std::env::home_dir()
            .expect("User's home directory should always exist")
            .join(".config/concmd/config.toml"),
    };
    Config::read_config(&config_location)
}

#[cfg(target_family = "windows")]
fn get_config() -> Result<Config> {
    let mut home_dir = std::env::home_dir().expect("home dir should always exist");
    println!("{:?}", home_dir);
    home_dir.push("AppData\\Roaming\\concmd\\config.toml");

    println!("{:?}", home_dir);

    Config::read_config(&home_dir)
}

// Types that impl HasAttr have a name and ID to display e.g. pages, spaces
fn render_name_id_list<A: HasAttr>(items: &[A]) {
    for i in items.iter() {
        println!("ID: {}, Title: {}", i.get_id(), i.get_name())
    }
}

fn get_path(path: String) -> Result<PathBuf> {
    if cfg!(target_family = "unix") {
        Ok(expanduser::expanduser(path)?)
    } else {
        Ok(PathBuf::from(path))
    }
}
