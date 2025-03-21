mod actions;
mod conf_api;
mod tui;

use anyhow::{Context, Result};
use cursive::Cursive;
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
}

// Config structure. Note deserialize_with for save_location, see fn
#[derive(Deserialize, Debug, Clone)]
struct Config {
    #[serde(deserialize_with = "from_tilde_path")]
    save_location: PathBuf,
    #[serde(default, deserialize_with = "from_tilde_path_optional")]
    history_location: Option<PathBuf>,
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

fn from_tilde_path<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    expanduser::expanduser(s).map_err(D::Error::custom)
}

fn main() {
    let config = get_config();

    let cli = Args::parse();

    match &cli.action {
        Action::Edit { id, last } => {
            if *last {
                actions::edit_last_page(&config)
            } else {
                // ID will always be present here, but check is required
                if let Some(id) = id {
                    actions::edit_page(&config, &id);
                }
            }
        }
        Action::View => {
            let mut siv = Cursive::default();
            siv.set_user_data(config);
            crate::tui::display(&mut siv)
        }
    }
}

fn get_config() -> Config {
    let mut home_dir = home::home_dir().expect("home dir should always exist");
    home_dir.push(".config/concmd/config.toml");

    Config::read_config(&home_dir).unwrap()
}
