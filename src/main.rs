mod actions;
mod alt_tui;
mod conf_api;
mod tui;

use anyhow::{Context, Result};
use serde::Deserialize;

#[cfg(target_family = "unix")]
use serde::{Deserializer, de::Error};

use std::fs::File;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

use clap::{ArgGroup, Parser};

// Command line interface for clap
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    // Require either the id or the --last arg
    #[clap(group(ArgGroup::new("edit_mode").required(true).args(&["id", "last"])))]
    Edit {
        #[arg(long)]
        last: bool,
        #[arg(short, long)]
        id: Option<String>,
        #[arg(short, long)]
        preview: Option<u16>,
    },
    View,
    New {
        #[arg(long, short)]
        path: Option<String>,
        #[arg(long, short)]
        title: String,
        #[arg(long, short)]
        edit: bool,
    },
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
    tui: Option<Tui>,
}

#[cfg(target_family = "windows")]
#[derive(Deserialize, Debug, Clone)]
struct Config {
    save_location: PathBuf,
    history_location: Option<PathBuf>,
    auto_sync: Option<bool>,
    api: Api,
    editor: Option<Editor>,
    tui: Option<Tui>,
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
#[serde(rename_all = "lowercase")]
enum Tui {
    Ratatui,
    Cursive,
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

fn main() {
    let config = match get_config() {
        Ok(c) => c,
        Err(e) => {
            println!("ERROR: Error fetching config: {}", e);
            return;
        }
    };

    let cli = Args::parse();

    match cli.action {
        Action::Edit { id, last, preview } => {
            // FIXME: this logic sucks, refactor pls. Also says "page uploaded" when it wasn't
            // also maybe allow the user to give the preview length as an optional param

            let result =
                match (last, id, preview) {
                    (true, None, Some(preview)) => {
                        actions::get_last_page_preview(&config, preview as usize)
                            .map(|s| println!("{}", s))
                    }
                    (true, None, None) => actions::edit_last_page(&config)
                        .map(|_| println!("Page edited successfully!")),
                    (false, Some(id), Some(preview)) => {
                        actions::get_page_preview_by_id(&config, &id, preview as usize)
                            .map(|s| println!("{}", s))
                    }
                    (false, Some(id), None) => actions::edit_id(&config, &id)
                        .map(|_| println!("Page edited successfully!")),
                    _ => panic!("Unsupported CLI arguments set"),
                };

            if let Err(e) = result {
                if e.to_string() == "USER_CANCEL" {
                    println!("Exited without syncing changes")
                } else {
                    eprintln!("ERROR: {}", e)
                }
            }
        }
        Action::View => match config.tui {
            Some(Tui::Cursive) => match actions::view_pages(&config) {
                Ok(_) => println!("Page edited successfully!"),
                Err(e) if e.to_string() == "USER_CANCEL" => {
                    println!("Exited without saving changes")
                }
                Err(e) if e.to_string() == "USER_APP_EXIT" => {
                    println!("Exited without selecting a page")
                }
                Err(e) => println!("ERROR: {}", e),
            },
            Some(Tui::Ratatui) | None => match actions::view_pages(&config) {
                Ok(_) => {}
                Err(e) if e.to_string() == "USER_APP_EXIT" => {
                    println!("Exited without selecting a page")
                }
                Err(e) => println!("ERROR: {}", e),
            },
        },
        Action::New { path, title, edit } => {
            let result = if let Some(path) = path {
                #[cfg(target_family = "unix")]
                let expanded_path = match expanduser::expanduser(path) {
                    Ok(ex_path) => ex_path,
                    Err(_) => {
                        println!("The provided path is not valid");
                        return;
                    }
                };
                #[cfg(target_family = "windows")]
                let expanded_path = PathBuf::from(path);

                actions::cli_new_page(&config, &edit, title.clone(), Some(&expanded_path))
                    .map(|_| println!("New page created"))
            } else {
                actions::cli_new_page(
                    &config,
                    &edit,
                    title.clone(),
                    path.map(PathBuf::from).as_deref(),
                )
            };

            if let Err(e) = result {
                if e.to_string() == "USER_CANCEL" {
                    println!("Exited without saving changes");
                } else if e.to_string().starts_with("A page with this title") {
                    eprintln!(
                        "ERROR: A page with title \"{}\" already exists in this space",
                        title
                    )
                } else {
                    eprintln!("ERROR: {}", e)
                }
            }
        }
    }
}

// Helper function to add the home dir to the config path. Config is always expected to live in the
// ~/.config/concmd directory.
#[cfg(target_family = "unix")]
fn get_config() -> Result<Config> {
    let mut home_dir = dirs::home_dir().ok_or(anyhow::anyhow!("Home dir could not be found"))?;
    home_dir.push(".config/concmd/config.toml");

    Config::read_config(&home_dir)
}

#[cfg(target_family = "windows")]
fn get_config() -> Result<Config> {
    let mut home_dir = dirs::home_dir().expect("home dir should always exist");
    println!("{:?}", home_dir);
    home_dir.push("AppData\\Roaming\\concmd\\config.toml");

    println!("{:?}", home_dir);

    Config::read_config(&home_dir)
}
