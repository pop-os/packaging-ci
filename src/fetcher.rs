use crate::{
    config::{Config, ConfigOrganization},
    git,
    github::{self, Branch as GitHubBranch, Repo},
};

use futures::{
    prelude::*,
    stream::{FuturesUnordered, Stream},
};
use reqwest::Client;
use std::{collections::HashMap, io, path::Path, rc::Rc, sync::Arc};

#[derive(Debug, Error)]
pub enum Error {
    #[error("github")]
    FetchRemote(Box<str>, #[source] github::Error),
    #[error("failed to fetch repos from GitHub organization {}", _0)]
    FetchOrgRepos(Box<str>, #[source] github::Error),
    #[error("failed to checkout git branch for {}", _0)]
    GitCheckout(Box<str>, #[source] io::Error),
    #[error("failed to clone {}", _0)]
    GitClone(Box<str>, #[source] io::Error),
    #[error("failed to fetch {}", _0)]
    GitFetch(Box<str>, #[source] io::Error),
    #[error("failed to get status of branches for {}", _0)]
    GitStatus(Box<str>, #[source] io::Error),
}

#[derive(Debug)]
pub struct Repository {
    // The repository which has been fetched.
    pub name: Box<str>,
    /// The working directory of this repository.
    pub directory: Box<Path>,
    // Branches found in this repository
    pub branches: Box<[Branch]>,
}

#[derive(Debug, Clone)]
pub struct Branch {
    /// The branch which has been checked.
    pub name: Box<str>,
    /// The commit hash for this branch from the remote.
    pub sha: Box<str>,
    /// If this branch was just checked out.
    pub required_checkout: bool,
}

pub struct Fetcher<'a> {
    client: &'a Arc<Client>,
    config: &'a Config,
}

impl<'a> Fetcher<'a> {
    pub fn new(client: &'a Arc<Client>, config: &'a Config) -> Self {
        Self { client, config }
    }

    /// Fetches an organization's repositories asynchronously.
    pub async fn organization(&self, org: &str) -> Result<Vec<Repo>, Error> {
        github::organization_repos(self.client.clone(), org)
            .await
            .map_err(|why| Error::FetchOrgRepos(org.into(), why))
    }

    /// Fetches multiple repositories and their branches concurrently.
    pub fn repos<'b>(
        &'b self,
        org: &'b ConfigOrganization,
        repos: &'b [Repo],
    ) -> impl Stream<Item = Result<Repository, Error>> + 'b {
        repos
            .into_iter()
            .filter(|repo| repo_filter(org, repo))
            .map(move |repo| self.branches(&org.name, repo))
            .collect::<FuturesUnordered<_>>()
    }

    /// Fetches the branches of a repository concurrently
    pub async fn branches<'b>(
        &'b self,
        user: &'b str,
        repo: &'b Repo,
    ) -> Result<Repository, Error> {
        let Self { client, config } = *self;
        let cwd = config.dirs.base.join(&*repo.name);

        let remote_branches = async {
            fetch_remote_branches(client.clone(), user, &*repo.name)
                .await
                .map_err(|why| Error::FetchRemote(repo.name.clone(), why))
        };

        let local_branches = fetch_local_branches(&config.dirs.base, &cwd, user, &*repo.name);

        info!(
            "fetching local and remote branches for {}/{}",
            user, repo.name
        );
        let (remote_branches, local_branches) = try_join!(remote_branches, local_branches)?;
        info!(
            "fetched local and remote branches for {}/{}",
            user, repo.name
        );

        let mut branches = Vec::new();

        // NOTE: This must be executed serially, rather than concurrently.
        //       Concurrent executions of git in the same directory causes
        //       git to get into an inconsistent state.
        let mut fetched = false;
        for branch in remote_branches {
            let required_checkout = local_branches
                .get(&branch.name)
                .map_or(true, |commit| commit != &branch.commit.sha);

            if required_checkout {
                if !fetched {
                    fetched = true;
                    info!("fetching on {}", repo.name);
                    if let Err(why) = git::fetch(&cwd, "origin").await {
                        let repo = [&repo.name, "/", &branch.name].concat();
                        let error = Error::GitFetch(repo.into(), why);
                        return Err(error);
                    }
                }

                info!("checking out {}: {}", repo.name, branch.name);
                if let Err(why) = git::checkout_id(&cwd, &branch.commit.sha).await {
                    let repo = [&repo.name, "/", &branch.name].concat();
                    let error = Error::GitCheckout(repo.into(), why);
                    return Err(error);
                }
                info!("checked out {}: {}", repo.name, branch.name);
            }

            branches.push(Branch {
                name: branch.name,
                sha: branch.commit.sha,
                required_checkout,
            });
        }

        branches.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(Repository {
            directory: cwd.into(),
            name: repo.name.clone(),
            branches: branches.into(),
        })
    }
}

async fn fetch_local_branches(
    parent_cwd: &Path,
    cwd: &Path,
    org: &str,
    repo: &str,
) -> Result<HashMap<Box<str>, Box<str>>, Error> {
    if !cwd.exists() {
        info!("cloning {}/{}", org, repo);
        let url = ["https://github.com/", org, "/", repo].concat();
        git::clone(parent_cwd, &url)
            .await
            .map_err(|why| Error::GitClone([org, "/", repo].concat().into(), why))?;
        info!("cloned {}/{}", org, repo);
    }

    git::local_branch_and_ids(&cwd)
        .await
        .map_err(|why| Error::GitStatus([org, "/", repo].concat().into(), why))
}

async fn fetch_remote_branches(
    client: Arc<Client>,
    org: &str,
    repo: &str,
) -> Result<Vec<GitHubBranch>, github::Error> {
    let mut branches = github::repository_branches(client, org, repo).await;

    // Filter `_nobuild` branches.
    if let Ok(branches) = branches.as_mut() {
        let mut remove = Vec::new();

        for (id, branch) in branches.iter().enumerate() {
            if branch.name.contains("/") {
                remove.push(id);
            }
        }

        for id in remove.into_iter().rev() {
            branches.swap_remove(id);
        }
    }

    branches
}

/// Filter repositories which meet filtering criteria.
fn repo_filter(org: &ConfigOrganization, repo: &Repo) -> bool {
    if let Some(needle) = org.starts_filter.as_ref() {
        if repo.name.starts_with(needle.as_ref()) {
            return false;
        }
    }

    true
}
