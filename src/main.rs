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

use clap::Parser;

// Command line interface for clap
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    Edit {
        #[arg(short, long)]
        id: String,
    },
    View,
}

// Config structure. Note deserialize_with for save_location, see fn
#[derive(Deserialize, Debug)]
struct Config {
    #[serde(deserialize_with = "from_tilde_path")]
    save_location: PathBuf,
    api: Api,
    editor: Option<String>,
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

#[derive(Deserialize, Debug)]
struct Api {
    confluence_domain: String,
    username: String,
    token: String,
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

fn main() {
    let config = get_config();

    let cli = Args::parse();

    match &cli.action {
        Action::Edit { id } => crate::actions::edit_page(&config, id),
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
