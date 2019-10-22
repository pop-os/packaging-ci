//! Generic error types shared across the project

use std::{io, path::Path};

#[derive(Debug, Error)]
pub enum DirError {
    #[error("unable to create directory {}", dir.display())]
    Create {
        dir: Box<Path>,
        #[source]
        source: io::Error,
    },
    #[error("unable to remove {}", dir.display())]
    Remove {
        dir: Box<Path>,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Error)]
pub enum FileError {
    #[error("unable to create file {}", file.display())]
    CreateFile {
        file: Box<Path>,
        #[source]
        source: io::Error,
    },
    #[error("unable to create file {}", file.display())]
    WriteFile {
        file: Box<Path>,
        #[source]
        source: io::Error,
    },
}
