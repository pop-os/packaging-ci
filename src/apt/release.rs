use crate::{errors::Error, misc::create_and_write, Series};
use markup::raw;
use std::path::Path;

markup::define! {
    ReleaseFileTemplate<'a>(arch: &str, context: &'a str, description: &'a str, codename: &'a str, version: &'a str, pocket: &'a str) {
        "Archive: " { raw(codename) } "\n"
        "Version: " { raw(version) } "\n"
        "Component: main\n"
        "Origin: " { raw(context)} "-" { raw(pocket)} "\n"
        "Label: " { raw(description) } " " { raw(pocket)} "\n"
        "Architecture: " { raw(arch) } "\n"
    }
}

pub async fn generate(file: &Path, arch: &str, context: &str, description: &str, pocket: &str, codename: &str, version: &str) -> Result<(), Error> {
    create_and_write(file, format!("{}", ReleaseFileTemplate { arch, context, description, codename, version, pocket }).as_bytes())
        .await?;

    Ok(())
}
