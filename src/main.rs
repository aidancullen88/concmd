mod actions;
mod conf_api;

use core::panic;
use serde::{de::Error, Deserialize, Deserializer};
use std::fs::File;
use std::{io::Read, path::PathBuf};
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
    key: Key,
}

#[derive(Deserialize, Debug)]
struct Key {
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
    let mut file = match File::open(home_dir) {
        Ok(file) => file,
        Err(err) => {
            panic!("{}", err)
        }
    };

    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("toml file should always be readable");
    let config: Config = toml::from_str(contents.as_str()).unwrap();

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
        Action::Edit { id } => crate::actions::edit_page_by_id(&config, id),
    }
}
