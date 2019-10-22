use crate::{config::Config, fetcher::Repository, git::GitTar, STRING_BUF};

use futures::{prelude::*, stream::FuturesUnordered};

use std::collections::HashMap;

/// Collates the build queue, and all of its required information.
pub async fn build_queue<'a>(
    config: &'a Config,
    repo: &'a Repository,
) -> HashMap<&'a str, HashMap<&'a str, GitTar>> {
    let mut build_queue = HashMap::<&'a str, HashMap<&'a str, GitTar>>::new();

    for series in config.series.keys() {
        build_queue.insert(&series, HashMap::new());
    }

    let git_dir = &config.dirs.git;

    let &Repository {
        ref branches,
        ref name,
        ref directory,
    } = repo;

    // Concurrently generate git tar archives for each branch
    let mut stream = branches
        .iter()
        .map(|branch| {
            async move {
                info!("{} commit {}: building git tar", name, branch.sha);

                let archive_path = STRING_BUF.with(|buffer| {
                    let mut buffer = buffer.borrow_mut();
                    buffer.clear();
                    buffer.push_str(&branch.sha);
                    buffer.push_str(".tar");
                    git_dir.join(&*buffer)
                });

                let git_tar = GitTar::new(directory, &archive_path, &branch.sha)
                    .await
                    .unwrap();

                (branch, git_tar)
            }
        })
        .collect::<FuturesUnordered<_>>();

    // Collate the information as it is received from the stream.
    while let Some((branch, git_tar)) = stream.next().await {
        let (pocket, codename) = parse_branch(&branch.name);

        match codename {
            Some(codename) => {
                build_queue.entry(&codename).and_modify(|pockets| {
                    pockets.insert(pocket, git_tar);
                });
            }
            None => {
                for pockets in build_queue.values_mut() {
                    pockets.entry(&pocket).or_insert_with(|| git_tar.clone());
                }
            }
        }
    }

    build_queue
}

fn parse_branch(branch: &str) -> (&str, Option<&str>) {
    let mut parts = branch.split('_');
    let pocket = parts.next().expect("expected a branch pocket");
    (pocket, parts.next())
}
