use anyhow::Context;
use std::path::Path;
use tokio::{
    fs::{self, File},
    io::AsyncReadExt,
};

pub async fn fetch<'a>(
    buffer: &'a mut String,
    path: &Path,
    retry: bool,
) -> anyhow::Result<(File, Vec<(&'a str, &'a str)>)> {
    if retry || !path.exists() {
        let file = File::create(path)
            .await
            .context("failed to create blacklist file")?;
        return Ok((file, Vec::new()));
    }

    let mut file = fs::OpenOptions::new()
        .write(true)
        .read(true)
        .open(path)
        .await
        .context("failed to open blacklist file")?;

    file.read_to_string(buffer)
        .await
        .context("failed to read blacklist file to string")?;

    let entries = buffer
        .lines()
        .map(|line| {
            if let Some(pos) = line.find(' ') {
                let (first, second) = line.split_at(pos);
                Ok((first, &second[1..]))
            } else {
                Err(())
            }
        })
        .collect::<Result<_, ()>>();

    match entries {
        Ok(entries) => Ok((file, entries)),
        Err(why) => {
            drop(file);

            error!("the blacklist file was corrupted, and is now being recreated");
            let file = File::create(path)
                .await
                .context("failed to create blacklist file")?;
            return Ok((file, Vec::new()));
        }
    }
}
