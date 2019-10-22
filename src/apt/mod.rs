pub mod release;

use crate::{
    config::Config,
    misc::{check_call, check_output},
};

use tokio::fs;

pub async fn create_dist(config: &Config, pocket: &str, codename: &str, version: &str) -> io::Result<()> {
    let pocket_dir = config.dirs.pocket.join(pocket);
    let dist_dir = pocket_dir.join("dists").join(codename);
    let dist_release = dist_dir.join("Release");
    let comp_dir = dist_dir.join("main");
    let source_dir = comp_dir.join("source");
    let sources_file = source_dir.join("Sources");
    let sources_release = source_dir.join("Release");
    let context = &config.context.replace("/", "-");
    let description = &config.description;

    let pool = ["pool/", codename].concat();

    fs::create_dir_all(source_dir).await?;

    let source = generate_source_directory(&pool, &pocket_dir).await?;

    fs::write(&sources_file, source).await?;

    check_call("gzip", &["--keep", sources_file.to_str().unwrap()], None).await?;

    release::generate(&sources_releases, "source", context, description, pocket, codename, version).await?;

    let mut binary_file = fs::OpenOptions::new().append(true).open(&binary_packages).await?;

    for build_arch in config.archs.keys() {
        let binary_dir = comp_dir.join(&["binary-", build_arch].concat());
        let binary_packages = binary_dir.join("Packages");
        let binary_release = binary_dir.join("Release");

        fs::create_dir(binary_dir).await?;

        let packages = generate_binary_directory(build_arch, &pool, &pocket_dir).await?;

        fs::write(&binary_packages, packages).await?;

        check_call("gzip", &["--keep", binary_packages.to_str().unwrap()], None).await?;

        release::generate(&binary_release, build_arch, context, description, pocket, codename, version).await?;
    }

    let build_archs = config.archs.keys().join(" ");
    let release = dist_release(dist_dir.to_str().unwrap(), &build_archs, context, description, pocket, codename).await?;

    fs::write(&dist_release, release).await?;

    let dist_dir = dist_dir.to_str().unwrap();
    gpg_inrelease(dist_dir, &config.email).await?;
    gpg_release(dist_dir, &config.email).await?;
}

async fn generate_source_directory(pool: &str, pocket_dir: &Path) -> io::Result<String> {
    check_output("apt-ftparchive", &["-qq", "sources", pool], Some(pocket_dir)).await
}

async fn generate_binary_directory(build_arch: &str, pool: &str, pocket_dir: &Path) -> io::Result<String> {
    check_output("apt-ftparchive", &[
        "--arch", build_arch,
        "packages", pool,
    ], Some(pocket_dir)).await
}

async fn dist_release(dist_dir: &Path, build_archs: &str, context: &str, description: &str, pocket: &str, codename: &str, version: &str) -> io::Result<String> {
    check_output("apt-ftparchive", &[
        "-o", &["APT::FTPArchive::Release::Origin=", context, "-", pocket].concat(),
        "-o", &["APT::FTPArchive::Release::Label=", description, " ", pocket].concat(),
        "-o", &["APT::FTPArchive::Release::Suite=", codename].concat(),
        "-o", &["APT::FTPArchive::Release::Version=", version].concat(),
        "-o", &["APT::FTPArchive::Release::Codename=", codename].concat(),
        "-o", &["APT::FTPArchive::Release::Architectures=", build_archs].concat(),
        "-o", "APT::FTPArchive::Release::Components=main",
        "-o", &["APT::FTPArchive::Release::Description=Pop!_OS Staging ", codename, " ", version, " ", pocket].concat()
        "release", "."
    ], Some(dist_dir)).await
}

async fn gpg_inrelease(dist_dir: &str, email: &str) -> io::Result<()> {
    check_call("gpg", &[
        "--clearsign",
        "--local-user", email,
        "--batch", "--yes",
        "--digest-algo", "sha512",
        "-o", &[dist_dir, "/InRelease"].concat(),
        &[dist_dir, "/Release"].concat(),
    ], None).await
}

async fn gpg_release(dist_dir: &str, email: &str) -> io::Result<()> {
    check_call("gpg", &[
        "-abs",
        "--local-user", email,
        "--batch", "--yes",
        "--digest-algo", "sha512",
        "-o", &[dist_dir, "/Release.gpg"].concat(),
        &[dist_dir, "/Release"].concat(),
    ], None).await
}
