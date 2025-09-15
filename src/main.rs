mod actions;
mod alt_tui;
mod conf_api;
mod tui;

use anyhow::{Context, Result};
use serde::{de::Error, Deserialize, Deserializer};
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
    #[clap(group(
        ArgGroup::new("edits")
        .multiple(false)
        .required(true)
        .args(&["id", "last"]),
    ))]
    Edit {
        #[arg(long, action)]
        last: bool,
        #[arg(short, long)]
        id: Option<String>,
    },
    View,
    Upload {
        #[arg(long, short)]
        path: String,
        #[arg(long, short)]
        title: String,
        #[arg(long, short)]
        edit: bool,
    },
    New {
        #[arg(long, short)]
        title: String,
        #[arg(long, short)]
        edit: bool,
    },
}

// Config structure. Note deserialize_with for save_location, see fn
// Deserialisation for history location requires a different function to deal with the optional
// case
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
// expands the tilde to the users home directory (unix only)
fn from_tilde_path<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    expanduser::expanduser(s).map_err(D::Error::custom)
}

// Same as the above deserialiser but handles the optional case for history_location
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

// Collects user errors that require creating a new file to be aborted
// #[derive(Debug)]
// enum UserErrors {
//     InvalidSavePath,
//     TitleExists,
// }
//
// impl std::error::Error for UserErrors {}
//
// impl std::fmt::Display for UserErrors {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         match self {
//             UserErrors::InvalidSavePath => {
//                 write!(f, "The save_location defined in the config is invalid. Please edit the config and try again")
//             }
//             UserErrors::TitleExists => {
//                 write!(f, "The title chosen already exists. Please choose another title and try again, or use 'concmd view' to find and edit the existing page")
//             }
//         }
//     }
// }

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
        Action::Edit { id, last } => {
            let result = if last {
                actions::edit_last_page(&config)
            } else {
                // ID will always be present here, but check is required
                if let Some(id) = id {
                    actions::edit_id(&config, &id)
                } else {
                    panic!("ID cannot be missing if last is false!")
                }
            };
            match result {
                Ok(_) => println!("Page edited successfully!"),
                Err(e) if e.to_string() == "USER_CANCEL" => {
                    println!("Exited without syncing changes")
                }
                Err(e) => println!("ERROR: {}", e),
            }
        }
        Action::View => match actions::view_pages(&config) {
            Ok(_) => println!("Page edited successfully!"),
            Err(e) if e.to_string() == "USER_CANCEL" => {
                println!("Exited without saving changes")
            }
            Err(e) if e.to_string() == "USER_APP_EXIT" => {
                println!("Exited without selecting a page")
            }
            Err(e) => println!("ERROR: {}", e),
        },
        Action::Upload { path, title, edit } => {
            let expanded_path = match expanduser::expanduser(path) {
                Ok(ex_path) => ex_path,
                Err(_) => {
                    println!("The provided path is not valid");
                    return;
                }
            };
            match actions::upload_existing_page(&config, &edit, &expanded_path, title) {
                Ok(_) => println!("New page successfully created!"),
                Err(e) if e.to_string() == "USER_CANCEL" => {
                    println!("Exited without saving changes")
                }
                Err(e) => println!("ERROR: {}", e),
            };
        }
        Action::New { title, edit } => {
            match actions::create_new_page(&config, &edit, title.clone()) {
                Ok(_) => println!("New page successfully created!"),
                Err(e) if e.to_string() == "USER_CANCEL" => {
                    println!("Exited without saving changes")
                }
                Err(e) if e.to_string().starts_with("A page with this title") => {
                    println!(
                        "ERROR: A page with title \"{}\" already exists in this space",
                        title
                    )
                }
                Err(e) => println!("ERROR: {}", e),
            }
        }
    }
}

// Helper function to add the home dir to the config path. Config is always expected to live in the
// ~/.config/concmd directory.
fn get_config() -> Result<Config> {
    let mut home_dir = home::home_dir().expect("home dir should always exist");
    home_dir.push(".config/concmd/config.toml");

    Config::read_config(&home_dir)
}
