use chrono::{DateTime, Utc};
use numtoa::NumToA;
use once_cell::sync::OnceCell;
use reqwest::Client;
use serde::de::DeserializeOwned;
use std::{fs, path::Path, sync::Arc};

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to deserialize JSON response")]
    Deserialize(#[source] reqwest::Error),
    #[error("failed to get organization repositories for {}", org)]
    GetOrgRepos {
        org: Box<str>,
        #[source]
        source: reqwest::Error,
    },
    #[error("failed to get repo branches for {}", repo)]
    GetRepoBranches {
        repo: Box<str>,
        #[source]
        source: reqwest::Error,
    },
    #[error("failed to set status for {}", repo)]
    Status {
        repo: Box<str>,
        #[source]
        source: reqwest::Error,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Repo {
    pub name: Box<str>,
    pub url: Box<str>,
    pub pushed_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct Branch {
    pub name: Box<str>,
    pub commit: Commit,
}

#[derive(Debug, Deserialize)]
pub struct Commit {
    pub sha: Box<str>,
}

static GITHUB_TOKEN: OnceCell<Option<String>> = OnceCell::new();

fn github_token() -> Option<&'static str> {
    GITHUB_TOKEN
        .get_or_init(move || {
            if Path::new(TOKEN_PATH).exists() {
                Some(
                    fs::read_to_string(TOKEN_PATH)
                        .expect("failed to read token")
                        .trim()
                        .to_owned(),
                )
            } else {
                None
            }
        })
        .as_ref()
        .map(|s| s.as_str())
}

pub async fn organization_repos(client: Arc<Client>, org: &str) -> Result<Vec<Repo>, Error> {
    fetch_all::<Repo>(&client, &["/orgs/", org, "/repos"].concat()).await
}

pub async fn repository_branches(
    client: Arc<Client>,
    owner: &str,
    repo: &str,
) -> Result<Vec<Branch>, Error> {
    fetch_all::<Branch>(
        &client,
        &["/repos/", owner, "/", repo, "/branches"].concat(),
    )
    .await
}

#[derive(Debug, Serialize)]
pub struct StatusContext<'a> {
    context: &'a str,
    description: &'a str,
    state: &'a str,
    target_url: &'a str,
}

pub async fn status(
    client: &Client,
    owner: &str,
    repo: &str,
    id: &str,
    context: &StatusContext<'_>,
) -> Result<(), Error> {
    let mut url = [
        "https://api.github.com/repos/",
        owner,
        "/",
        repo,
        "/statuses/",
        id,
    ]
    .concat();

    if let Some(token) = github_token() {
        url.push_str("&access_token=");
        url.push_str(&*token);
    }

    client
        .post(&*url)
        .header("accept", "application/vnd.github.v3+json")
        .header("content-type", "application/json")
        .json(context)
        .send()
        .await
        .map_err(|source| Error::Status {
            repo: [owner, "/", repo].concat().into(),
            source,
        })?;

    Ok(())
}

const TOKEN_PATH: &str = ".github_token";

async fn fetch_all<T: DeserializeOwned>(client: &Client, url: &str) -> Result<Vec<T>, Error> {
    let mut data = Vec::new();
    let mut page = 0u32;
    let per_page = 100;
    let buf = &mut [0u8; 20];

    let mut page_url = String::from("https://api.github.com");
    page_url.push_str(url);
    page_url.push_str("?page=");

    let truncate_to = page_url.len();

    loop {
        page += 1;

        page_url.truncate(truncate_to);
        page_url.push_str(page.numtoa_str(10, buf));
        page_url.push_str("&per_page=");
        page_url.push_str(per_page.numtoa_str(10, buf));

        if let Some(token) = github_token() {
            page_url.push_str("&access_token=");
            page_url.push_str(&*token);
        }

        let page = client
            .get(&*page_url)
            .header("accept", "application/vnd.github.v3+json")
            .send()
            .await
            .map_err(|source| Error::GetOrgRepos {
                org: page_url.clone().into(),
                source,
            })?
            .error_for_status()
            .map_err(|source| Error::GetOrgRepos {
                org: page_url.clone().into(),
                source,
            })?
            .json::<Vec<T>>()
            .await
            .map_err(Error::Deserialize)?;

        let found = page.len();
        data.extend(page);
        if found < per_page {
            return Ok(data);
        }
    }
}
