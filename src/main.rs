#[macro_use]
extern crate futures;
#[macro_use]
extern crate log;

use pop_ci::{
    blacklist, collate,
    config::{Config, ConfigOrganization},
    dpkg,
    fetcher::{Fetcher, Repository},
    git::GitTar,
    misc, Error, STRING_BUF,
};

use anyhow::Context;
use futures::prelude::*;
use reqwest::Client;
use std::collections::HashMap;
use std::{env, error::Error as StdError, fmt::Write, ops::Deref, sync::Arc};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
};

// Fetch the blacklist entries while cleaning up the schroot sessions
async fn startup<'a>(
    config: &Config,
    buffer: &'a mut String,
) -> anyhow::Result<(File, Vec<(&'a str, &'a str)>)> {
    let blacklist_path = config.dirs.build.join("blacklist");

    let session_cleanup = async {
        misc::check_call("schroot", &["--end-session", "--all-sessions"], None)
            .await
            .context("failed to clean up schroot sessions")
    };

    let blacklist = blacklist::fetch(buffer, &blacklist_path, config.retry);

    try_join!(session_cleanup, blacklist).map(|r| r.1)
}

async fn main_() -> Result<(), anyhow::Error> {
    let config = Arc::new(Config::new()?);
    let client = Arc::new(Client::new());

    env::set_var("QUILT_PATCHES", "debian/patches");

    let blacklist_buffer = &mut String::new();
    let (mut blacklist_file, blacklisted) = startup(&config, blacklist_buffer).await?;
    let blacklisted: &[(&str, &str)] = &blacklisted;

    let fetcher = Fetcher::new(&client, &config);

    let (mut blacklist_tx, mut blacklist_rx) = unbounded_channel();

    let fetcher = async {
        for organization in &config.github.organizations {
            info!("fetching github organization: {}", organization.name);
            let repos = match fetcher.organization(&organization.name).await {
                Ok(repos) => repos,
                Err(why) => {
                    format_error(&why, |why| {
                        error!(
                            "failed to fetch GitHub organization {}: {}",
                            organization.name, why
                        );
                    });

                    continue;
                }
            };

            fetcher
                .repos(&organization, &repos)
                .for_each_concurrent(config.concurrent_builds, |result| {
                    let config = config.clone();
                    let client = client.clone();
                    let blacklist_tx = blacklist_tx.clone();

                    async move {
                        let repo = match result {
                            Ok(repo) => repo,
                            Err(why) => {
                                format_error(&why, |why| error!("fetching error: {}", why));
                                return;
                            }
                        };

                        process_repo(
                            &config,
                            &client,
                            organization,
                            repo,
                            blacklisted,
                            blacklist_tx,
                        )
                        .await;
                    }
                })
                .await;
        }
    };

    let mut buffer = String::new();

    let blacklist_writer = async move {
        while let Some((git_id, series)) = blacklist_rx.next().await {
            warn!("appending {} ({}) to blacklist", git_id, series);

            buffer.clear();
            buffer.push_str(&git_id);
            buffer.push(' ');
            buffer.push_str(&series);
            buffer.push('\n');

            if let Err(why) = blacklist_file.write_all(buffer.as_bytes()).await {
                error!("failed to write {} to blacklist", git_id);
            }
        }
    };

    // Runs the fetcher and blacklist writer at the same time.
    join!(fetcher, blacklist_writer);

    Ok(())
}

async fn process_repo(
    config: &Config,
    client: &Arc<Client>,
    org: &ConfigOrganization,
    repo: Repository,
    blacklisted: &[(&str, &str)],
    mut blacklist: UnboundedSender<(Box<str>, Box<str>)>,
) -> Result<(), Error> {
    let build_queue = collate::build_queue(&config, &repo).await;

    let mut deb_paths = Vec::new();

    for (series, pockets) in &build_queue {
        let release = &config.series[*series];
        for (pocket, git_tar) in pockets {
            if blacklisted.contains(&(&*git_tar.id, pocket)) {
                info!(
                    "{} commit {} on {}: skipping because it is blacklisted",
                    repo.name, git_tar.id, *series
                );
            }

            let dpkg = dpkg::Dpkg {
                config: &config,
                client: &client,
                repo: &repo,
                codename: *series,
                release: release,
                git: git_tar,
            };

            // Generate the source tarballs and dsc files
            match dpkg.source().await {
                Ok((dsc_path, tar_path, path_version)) => {
                    info!("building {}", dsc_path.display());

                    // For each supported arch, build debian packages from the source tarballs.
                    for (arch, &build_all) in &config.archs {
                        info!("building {} for {}", dsc_path.display(), arch);
                        match dpkg
                            .binary(&path_version, &dsc_path, &*arch, build_all)
                            .await
                        {
                            Ok(debs) => deb_paths.extend_from_slice(&debs),
                            Err(why) => {
                                error!(
                                    "{} commit {} on {}: failed to build binaries: {}",
                                    repo.name, git_tar.id, *series, why
                                );
                            }
                        }
                    }
                }
                Err(why) => {
                    error!(
                        "{} commit {} on {}: {}",
                        repo.name, git_tar.id, *series, why
                    );
                    let _ = blacklist
                        .send((git_tar.id.clone(), Box::from(*series)))
                        .await;
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    better_panic::install();
    setup_logger();

    if let Err(why) = main_().await {
        let why: Box<dyn StdError + 'static> = Box::from(why);
        format_error(&*why, |why| error!("CI errored: {}", why));
    }
}

fn format_error<F: FnOnce(&str)>(why: &(dyn StdError + 'static), func: F) {
    STRING_BUF.with(|buffer| {
        let mut buffer = buffer.borrow_mut();

        buffer.clear();
        let _ = writeln!(buffer, "{}", why);

        let mut cause = why.source();
        while let Some(error) = cause {
            let _ = writeln!(buffer, "    caused by: {}", error);
            cause = error.source();
        }

        func(&buffer);
    });
}

fn setup_logger() -> Result<(), fern::InitError> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message
            ))
        })
        .level(log::LevelFilter::Warn)
        .level_for("pop_ci", log::LevelFilter::Debug)
        .chain(std::io::stdout())
        .chain(fern::log_file("ci.log")?)
        .apply()?;
    Ok(())
}
