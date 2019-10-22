use crate::misc::*;
use std::{collections::HashMap, io, path::Path};

#[derive(Debug, Clone)]
pub struct GitTar {
    pub id: Box<str>,
    pub datetime: Box<str>,
    pub archive: Box<Path>,
    pub timestamp: Box<str>,
}

impl GitTar {
    pub async fn new<'a>(cwd: &Path, archive_path: &Path, sha: &'a str) -> io::Result<Self> {
        let ts = timestamp_id(cwd, sha);
        let dt = datetime_id(cwd, sha);

        let ar = async {
            if archive_path.exists() {
                info!(
                    "{} commit {}: git already built",
                    cwd.file_name().unwrap().to_str().unwrap(),
                    sha
                );
                Ok(())
            } else {
                archive_id(cwd, sha, archive_path.to_str().unwrap()).await?;
                Ok(())
            }
        };

        let (ts, dt, _) = try_join!(ts, dt, ar)?;

        Ok(Self {
            id: sha.into(),
            timestamp: ts.into(),
            datetime: dt.into(),
            archive: archive_path.into(),
        })
    }
}

pub async fn archive_id(cwd: &Path, id: &str, archive: &str) -> io::Result<String> {
    check_output(
        "git",
        &["archive", "--format", "tar", "-o", archive, id],
        Some(cwd),
    )
    .await
}

pub async fn fetch(cwd: &Path, remote: &str) -> io::Result<()> {
    check_call("git", &["fetch", "origin"], Some(cwd)).await
}

pub async fn checkout_id(cwd: &Path, id: &str) -> io::Result<()> {
    check_call("git", &["checkout", "--force", "--detach", id], Some(cwd)).await?;
    check_call("git", &["submodule", "sync", "--recursive"], Some(cwd)).await?;
    check_call(
        "git",
        &["submodule", "update", "--init", "--recursive"],
        None,
    )
    .await?;
    clean(cwd).await
}

pub async fn clean(cwd: &Path) -> io::Result<()> {
    check_call("git", &["clean", "-xfd"], Some(cwd)).await
}

pub async fn clone(cwd: &Path, repo: &str) -> io::Result<()> {
    check_call("git", &["clone", "--recursive", repo], Some(cwd)).await
}

pub async fn datetime_id(cwd: &Path, id: &str) -> io::Result<String> {
    check_output("git", &["log", "-1", "--pretty=format:%cD", id], Some(cwd))
        .await
        .map(|string| string.trim().to_owned())
}

pub async fn local_branch_and_ids(cwd: &Path) -> io::Result<HashMap<Box<str>, Box<str>>> {
    let output = check_output(
        "git",
        &["branch", "--format=%(refname:lstrip=2) %(objectname)"],
        Some(cwd),
    )
    .await?;

    let mut collected = HashMap::new();
    for line in output.lines() {
        let mut fields = line.split_whitespace();

        let branch = fields.next().expect("missing branch").into();
        let commit = fields.next().expect("missing commit").into();

        collected.insert(branch, commit);
    }

    Ok(collected)
}

pub async fn ids_and_branches(
    map: &mut HashMap<Box<str>, Vec<Box<str>>>,
    cwd: &Path,
) -> io::Result<()> {
    check_call("git", &["fetch", "origin"], Some(cwd)).await?;
    let output = check_output("git", &["ls-remote", "--heads", "origin"], Some(cwd)).await?;
    map.clear();

    const PREFIX: &str = "refs/heads/";

    for line in output.lines() {
        let mut columns = line.split('\t');

        let id = columns.next().expect("no ID");
        let rawbranch = columns.next().expect("no rawbranch");
        let branch = &rawbranch[PREFIX.len()..];

        map.entry(id.into())
            .and_modify(|e| e.push(branch.into()))
            .or_insert_with(|| vec![branch.into()]);
    }

    Ok(())
}

pub async fn timestamp_id(cwd: &Path, id: &str) -> io::Result<String> {
    check_output("git", &["log", "-1", "--pretty=format:%ct", id], Some(cwd))
        .await
        .map(|string| string.trim().to_owned())
}
