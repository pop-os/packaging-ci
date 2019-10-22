use crate::errors::DirError;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
pub struct ConfigDirs {
    pub base: PathBuf,
    pub binary: PathBuf,
    pub build: PathBuf,
    pub git: PathBuf,
    pub repo: PathBuf,
    pub source: PathBuf,
}

impl ConfigDirs {
    pub fn setup(self) -> Result<Self, DirError> {
        let mut dir: &Path = &self.git;
        fs::create_dir_all(dir).map_err(|source| DirError::Create {
            dir: dir.into(),
            source,
        })?;

        dir = &self.source;
        fs::create_dir_all(dir).map_err(|source| DirError::Create {
            dir: dir.into(),
            source,
        })?;

        dir = &self.binary;
        fs::create_dir_all(dir).map_err(|source| DirError::Create {
            dir: dir.into(),
            source,
        })?;

        dir = &self.repo;
        if dir.exists() {
            fs::remove_dir_all(dir).map_err(|source| DirError::Remove {
                dir: dir.into(),
                source,
            })?;
        }

        fs::create_dir_all(dir).map_err(|source| DirError::Create {
            dir: dir.into(),
            source,
        })?;

        Ok(self)
    }
}
