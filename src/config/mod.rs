mod dirs;

pub use self::dirs::ConfigDirs;

use crate::errors::DirError;
use std::{collections::HashMap, env, fs, io, path::Path};

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to create initial directories")]
    Directory(#[from] DirError),
    #[error("config.toml was not found in the current working directory")]
    NotFound,
    #[error("parsing error")]
    Parse(#[from] toml::de::Error),
    #[error("attempted to read config.toml, but failed")]
    Read(#[source] io::Error),
}

#[derive(Debug)]
pub struct Config {
    pub archs: HashMap<Box<str>, bool>,
    pub series: HashMap<Box<str>, ConfigSeries>,
    pub github: ConfigGitHub,
    pub email: Box<str>,
    pub fullname: Box<str>,
    pub context: Box<str>,
    pub description: Box<str>,
    pub build_url: Option<Box<str>>,
    pub dirs: ConfigDirs,
    pub concurrent_builds: usize,
    pub dev: bool,
    pub retry: bool,
}

impl Config {
    pub fn new() -> Result<Self, Error> {
        let config_path = Path::new("config.toml");
        if !config_path.exists() {
            return Err(Error::NotFound);
        }

        let raw = fs::read_to_string(config_path).map_err(Error::Read)?;
        let raw_config = toml::from_str::<RawConfig>(&raw)?;

        Ok(Self {
            archs: raw_config.archs,
            build_url: raw_config.build_url,
            context: raw_config.context,
            description: raw_config.description,
            series: raw_config.series,
            github: raw_config.github,
            email: raw_config.email,
            fullname: raw_config.fullname,
            concurrent_builds: raw_config.concurrent_builds,
            dev: check_env("PACKAGING_DEV"),
            retry: check_env("PACKAGING_RETRY"),
            dirs: {
                let base = env::current_dir().expect("unable to get working directory");
                let build = base.join("_build");

                let dirs = ConfigDirs {
                    base,
                    binary: build.join("binary"),
                    git: build.join("git"),
                    repo: build.join("repos"),
                    source: build.join("source"),
                    build,
                };

                dirs.setup()?
            },
        })
    }
}

fn check_env(key: &str) -> bool {
    env::var(key).map(|dev| dev == "1").unwrap_or(false)
}

#[derive(Debug, Deserialize, SmartDefault)]
struct RawConfig {
    pub archs: HashMap<Box<str>, bool>,
    pub series: HashMap<Box<str>, ConfigSeries>,
    pub github: ConfigGitHub,
    pub email: Box<str>,
    pub fullname: Box<str>,
    pub context: Box<str>,
    pub description: Box<str>,
    pub build_url: Option<Box<str>>,

    #[default = 1]
    pub concurrent_builds: usize,
}

#[derive(Debug, Default, Deserialize)]
pub struct ConfigGitHub {
    #[serde(default)]
    pub organizations: Vec<ConfigOrganization>,
    #[serde(default)]
    pub repos: Vec<Box<str>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ConfigOrganization {
    pub name: Box<str>,

    /// Filter repositories with names that start with
    #[serde(default)]
    pub starts_filter: Option<Box<str>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ConfigSeries {
    pub release: Box<str>,
    pub wildcard: bool,
}
