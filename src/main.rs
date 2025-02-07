mod actions;
mod conf_api;

use anyhow::{Context, Result};
use serde::{de::Error, Deserialize, Deserializer};
use std::fs::File;
use std::{
    io::Read,
    path::{Path, PathBuf},
};
use toml;

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
    Fetch {
        #[arg(short, long)]
        space: String,

        #[arg(short, long)]
        page: String,

        #[arg(short, long)]
        filename: PathBuf,
    },
    Publish {
        #[arg(short, long)]
        space: String,

        #[arg(short, long)]
        page: String,

        #[arg(short, long)]
        filename: PathBuf,
    },
    Edit {
        #[arg(short, long)]
        id: String,
    },
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
        let mut file = File::open(&file_name).context("Config file could not be found")?;
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
    let mut home_dir = home::home_dir().expect("home dir should always exist");
    home_dir.push(".config/concmd/config.toml");

    let config = Config::read_config(&home_dir).unwrap();

    let cli = Args::parse();

    match &cli.action {
        Action::Fetch {
            space,
            page,
            filename,
        } => crate::actions::fetch_page(space, page, filename),
        Action::Publish {
            space,
            page,
            filename,
        } => crate::actions::publish_page(space, page, filename),
        Action::Edit { id } => crate::actions::edit_page(&config, id),
    }
}
