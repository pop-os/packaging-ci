use crate::{
    config::{Config, ConfigSeries},
    fetcher::Repository,
    git::GitTar,
    github::{self, StatusContext},
    misc::{check_call, check_output},
};

use anyhow::Context;
use chrono::DateTime;
use debian_changelog::{r#async::append as changelog_append, Entry as ChangelogEntry};
use reqwest::Client;

use std::{
    collections::HashMap,
    env, io,
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::{
    fs::{self, File},
    io::{AsyncRead, AsyncReadExt},
};

pub struct Dpkg<'a> {
    pub config: &'a Config,
    pub client: &'a Arc<Client>,
    pub repo: &'a Repository,
    pub codename: &'a str,
    pub release: &'a ConfigSeries,
    pub git: &'a GitTar,
}

impl<'a> Dpkg<'a> {
    pub async fn binary(
        &self,
        path_version: &str,
        dsc_path: &Path,
        build_arch: &str,
        build_all: bool,
    ) -> anyhow::Result<Vec<Box<Path>>> {
        let &Self {
            config,
            client,
            repo,
            codename,
            release,
            git,
        } = self;

        let dsc = read_to_string(dsc_path)
            .await
            .context("failed to read dsc file")?;

        let (source_name, version, package_list) =
            parse_dsc(&dsc).context("failed to parse dsc file")?;

        let mut debs: Vec<Box<Path>> = Vec::new();
        let mut found_binaries = true;

        for package_line in package_list.trim().lines() {
            let mut parts = package_line.split_whitespace();

            let binary = parts
                .next()
                .context("failed to find binary field in package list")?;
            let kind = parts
                .next()
                .context("failed to find kind field in package list")?;

            // Filter packages which are not required for Linux.
            let linux_non_requirement = || {
                &*self.repo.name == "linux"
                    && (binary.ends_with("-dbgsym") || binary.starts_with("linux-udebs-"))
            };

            // Filter packages which are not required for systemd.
            let systemd_non_requirement =
                || &*self.repo.name == "systemd" && binary.ends_with("-udeb");

            if kind == "udeb" || linux_non_requirement() || systemd_non_requirement() {
                continue;
            }

            let archs = parts
                .skip(2)
                .next()
                .context("failed to find architectures field in package list")?
                .replace("arch=", "");

            for arch in archs.split(',') {
                let deb_arch = if arch == "any" || arch == "linux-any" || arch == build_arch {
                    build_arch
                } else if build_all && arch == "all" {
                    "all"
                } else {
                    continue;
                };

                let filename = [binary, "_", path_version, "_", deb_arch, ".deb"].concat();
                let deb_path = self.config.dirs.binary.join(&filename);

                if !deb_path.exists() {
                    found_binaries = false;
                }

                debs.push(deb_path.into());
            }
        }

        if debs.is_empty() {
            info!("debs is empty");
            return Ok(debs);
        }

        let logname = [source_name, "_", path_version, "_", build_arch, ".build"].concat();
        let build_log = self.config.dirs.binary.join(&logname);

        if found_binaries {
            info!(
                "{} commit {} on {}: binaries for {} already built",
                source_name, git.id, codename, build_arch
            );
        } else if build_log.exists() {
            info!(
                "{} commit {} on {}: binaries for {} already failed to build",
                source_name, git.id, codename, build_arch
            );
        } else {
            info!(
                "{} commit {} on {}: building binaries for {}",
                source_name, git.id, codename, build_arch
            );

            // github_status(name, git.id, series.codename + "/binary-" + build_arch, "pending")

            let (ppa_key, ppa_release, ppa_proposed) = if config.dev {
                (
                    ".ppa-dev.asc",
                    "system76-dev/stable",
                    "system76-dev/pre-stable",
                )
            } else {
                (".ppa.asc", "system76/pop", "system76/proposed")
            };

            let key_path = config.dirs.base.join(ppa_key);

            let mut sbuild_args: Vec<String> = vec![
                ["--arch=", build_arch].concat(),
                ["--dist=", codename].concat(),
                [
                    "--extra-repository=deb http://us.archive.ubuntu.com/ubuntu/ ",
                    codename,
                    "-updates main restricted universe multiverse",
                ]
                .concat(),
                [
                    "--extra-repository=deb-src http://us.archive.ubuntu.com/ubuntu/ ",
                    codename,
                    "-updates main restricted universe multiverse",
                ]
                .concat(),
                [
                    "--extra-repository=deb http://us.archive.ubuntu.com/ubuntu/ ",
                    codename,
                    "-security main restricted universe multiverse",
                ]
                .concat(),
                [
                    "--extra-repository=deb-src http://us.archive.ubuntu.com/ubuntu/ ",
                    codename,
                    "-security main restricted universe multiverse",
                ]
                .concat(),
                [
                    "--extra-repository=deb http://ppa.launchpad.net/",
                    ppa_release,
                    "/ubuntu ",
                    codename,
                    " main",
                ]
                .concat(),
                [
                    "--extra-repository=deb-src http://ppa.launchpad.net/",
                    ppa_release,
                    "/ubuntu ",
                    codename,
                    " main",
                ]
                .concat(),
                [
                    "--extra-repository=deb http://ppa.launchpad.net/",
                    ppa_proposed,
                    "/ubuntu ",
                    codename,
                    " main",
                ]
                .concat(),
                [
                    "--extra-repository=deb-src http://ppa.launchpad.net/",
                    ppa_proposed,
                    "/ubuntu ",
                    codename,
                    " main",
                ]
                .concat(),
                ["--extra-repository-key=", key_path.to_str().unwrap()].concat(),
            ];

            if build_all {
                sbuild_args.push("--arch-all".into());
            }

            sbuild_args.push(dsc_path.to_str().expect("dsc path is not UTF-8").into());

            info!("building {} with sbuild", repo.name);
            match check_call("sbuild", &sbuild_args, Some(&config.dirs.binary)).await {
                Ok(()) => {
                    info!(
                        "{} commit {} on {}: finished building binaries for {}",
                        source_name, git.id, codename, build_arch
                    );

                    // github_status(name, git.id, series.codename + "/binary-" + build_arch, "success")
                }
                Err(why) => {
                    //     github_status(name, git.id, series.codename + "/binary-" + build_arch, "failure")
                    // except Exception as ex_s:
                    //     print("\x1B[1m{} commit {} on {}: failed to report build failure: {!r}\x1B[0m\n".format(source_name, git.id, series.codename, ex_s))

                    let context = read_to_string(dbg!(&build_log))
                        .await
                        .unwrap_or_else(|why| format!("failed to read build log: {}", why));

                    return Err(anyhow!("{}: {}", why, context));
                }
            }
        }

        for deb_path in &debs {
            if !deb_path.exists() {
                info!(
                    "{} commit {} on {}: missing binary at {}",
                    source_name,
                    git.id,
                    codename,
                    deb_path.display()
                );
                return Ok(Vec::new());
            }
        }

        Ok(debs)
    }

    pub async fn source(&self) -> anyhow::Result<(PathBuf, PathBuf, Box<str>)> {
        let &Self {
            config,
            codename,
            release,
            git,
            ..
        } = self;

        let source_dir = &self.config.dirs.source;
        let extract_dir: &Path = &source_dir.join(&[&git.id, "_", codename].concat());
        let debian_path = extract_dir.join("debian");
        let patches_dir = debian_path.join("patches");

        let is_linux = &*self.repo.name == "linux";

        if extract_dir.is_dir() {
            fs::remove_dir_all(extract_dir)
                .await
                .context("failed to remove extract directory")?;
        }

        fs::create_dir_all(extract_dir)
            .await
            .context("failed to create extract directory")?;

        let archive = git.archive.as_ref().to_str().unwrap();
        check_call("tar", &["xf", archive], Some(extract_dir))
            .await
            .context("failed to extract git tar")?;

        ensure!(debian_path.is_dir(), "no debian dir");

        let control = read_to_string(&debian_path.join("control"))
            .await
            .context("failed to debian/control into memory")?;

        let source_name = parse_source_from_control(&control)
            .map_err(|_| anyhow!("failed to parse source from debian/control file"))?;

        let mut changelog_version = check_output(
            "dpkg-parsechangelog",
            &["--show-field", "Version"],
            Some(extract_dir),
        )
        .await
        .context("failed to fetch changelog version")?;

        changelog_version.pop();

        let version = [
            &*changelog_version,
            &*git.timestamp,
            &*release.release,
            &git.id[..7],
        ]
        .join("~");

        // if dev {
        //     version.push_str("dev");
        // }

        let path_version = version.split(':').last().expect("no path version");
        let dsc_path = source_dir.join(&*[source_name, "_", path_version, ".dsc"].concat());
        let tar_path = source_dir.join(&*[source_name, "_", path_version, ".tar.xz"].concat());

        if dsc_path.exists() && tar_path.exists() {
            info!(
                "{} commit {} on {}: source already built",
                source_name, git.id, codename
            );
        } else {
            info!(
                "{} commit {} on {}: building source",
                source_name, git.id, codename
            );

            if let Some(target_url) = config.build_url.as_ref() {
                let context_ctx = [codename, "/source"].concat();
                let context = [&config.context, "/", &context_ctx].concat();
                let description = [&config.description, " ", &context_ctx].concat();
                let state = "pending";

                // let ctx = StatusContext {
                //     context: &context,
                //     description: &description,
                //     state: &state,
                //     target_url: &target_url,
                // };

                // github::status(&client, org, &repo.name, &git.id, &ctx).await;
            }

            let changelog_path = if is_linux {
                extract_dir.join("debian.master/changelog")
            } else {
                debian_path.join("changelog")
            };

            changelog_append(
                &changelog_path,
                ChangelogEntry {
                    author: &config.fullname,
                    date: DateTime::parse_from_rfc2822(&git.datetime).unwrap().into(),
                    distributions: vec![codename],
                    email: &config.email,
                    package: &source_name,
                    version: &version,
                    changes: vec!["* Auto Build"],
                    metadata: cascade! {
                        HashMap::new();
                        ..insert("urgency", "medium");
                    },
                },
            )
            .await
            .context("failed to append entry to changelog")?;

            if patches_dir.exists() {
                info!(
                    "{} commit {} on {}: applying debian patches",
                    source_name, git.id, codename
                );
                check_call("quilt", &["push", "-a"], Some(&extract_dir))
                    .await
                    .context("failed to push quilt patches")?;
                info!(
                    "{} commit {} on {}: finished applying debian patches",
                    source_name, git.id, codename
                );
            }

            if is_linux {
                info!(
                    "{} commit {} on {}: updating changelog",
                    source_name, git.id, codename
                );
                check_call("fakeroot", &["debian/rules", "clean"], Some(&extract_dir))
                    .await
                    .context("failed to execute `fakeroot debian/rules clean`")?;
                info!(
                    "{} commit {} on {}: finished updating changelog",
                    source_name, git.id, codename
                );
            }

            //     with debuild_lock:

            match debuild(git, &extract_dir).await {
                Ok(()) => {
                    info!(
                        "{} commit {} on {}: finished building source",
                        source_name, git.id, codename
                    );
                    //     github_status(name, git.id, series.codename + "/source", "success")
                }
                Err(why) => {
                    let error =
                        source_failure(&git.id, source_name, path_version, &config.dirs.source)
                            .await;

                    //     try:
                    //         github_status(name, git.id, series.codename + "/source", "failure")
                    //     except Exception as ex_s:
                    //         print("\x1B[1m{} commit {} on {}: failed to report build failure: {!r}\x1B[0m\n".format(source_name, git.id, series.codename, ex_s))

                    return Err(error);
                }
            }
        }

        ensure!(dsc_path.exists(), "missing dsc: {}", dsc_path.display());
        ensure!(tar_path.exists(), "missing tar: {}", tar_path.display());

        Ok((dsc_path, tar_path, path_version.into()))
    }
}

async fn debuild(git: &GitTar, extract_dir: &Path) -> io::Result<()> {
    let source_date_epoch = ["SOURCE_DATE_EPOCH=", &git.timestamp.to_string()].concat();
    let args = &[
        "--preserve-envvar",
        "PATH",
        "--set-envvar",
        &source_date_epoch,
        "--no-tgz-check",
        "-d",
        "-S",
        "--source-option=--tar-ignore=.git",
    ];

    check_call("debuild", args, Some(extract_dir)).await
}

async fn source_failure(
    id: &str,
    source_name: &str,
    path_version: &str,
    source: &Path,
) -> anyhow::Error {
    let log_name = [source_name, "_", path_version, "_source.build"].concat();
    let log_path = source.join(&log_name);

    match read_to_string(&log_path).await {
        Ok(log) => anyhow!("failed to build source:\n{}", log),
        Err(why) => anyhow!("failed to build source (log read failed)"),
    }
}

fn parse_dsc<'a>(dsc: &'a str) -> anyhow::Result<(&'a str, &'a str, &'a str)> {
    let (mut source, mut version, mut package_list) = ("", "", "");

    let mut set = 0;
    let mut read = 0;
    let mut lines = dsc.lines();
    while let Some(line) = lines.next() {
        read += line.len() + 1;

        if source.is_empty() && line.starts_with("Source:") {
            source = line[7..].trim();
            set += 1;
        } else if version.is_empty() && line.starts_with("Version:") {
            version = line[8..].trim();
            set += 1;
        } else if package_list.is_empty() && line.starts_with("Package-List:") {
            let start = read;

            while let Some(line) = lines.next() {
                if line.starts_with(' ') {
                    read += line.len() + 1;
                } else {
                    break;
                }
            }

            package_list = &dsc[start..read - 1];
            set += 1;
        }

        if set == 3 {
            return Ok((source, version, package_list));
        }
    }

    ensure!(!source.is_empty(), "missing source");
    ensure!(!version.is_empty(), "missing version");
    ensure!(!package_list.is_empty(), "missing package list");

    panic!("despite meeting requirements, failed to return dsc fields");
}

fn parse_source_from_control(control: &str) -> Result<&str, ()> {
    for line in control.lines() {
        if line.starts_with("Source:") {
            return Ok(line[7..].trim());
        }
    }

    Err(())
}

async fn read_to_string(path: &Path) -> io::Result<String> {
    let mut buffer = String::new();
    File::open(path).await?.read_to_string(&mut buffer).await?;
    Ok(buffer)
}
