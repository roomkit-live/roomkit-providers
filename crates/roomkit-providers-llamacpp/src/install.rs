//! Verify every release archive before unpacking it into a private staging directory.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use flate2::read::GzDecoder;
use roomkit_ai::ProviderError;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::{
    binary::{Release, io_error},
    error,
};

pub(crate) async fn build(
    release: &Release,
    variant: &str,
    target: &Path,
) -> Result<(), ProviderError> {
    let tls = roomkit_providers_http::tls_config()
        .map_err(|_| error("cannot configure TLS for the release download"))?;
    let http = reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .https_only(true)
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|_| error("cannot build the release download client"))?;
    let base = format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{}",
        release.build
    );
    install_from(&http, &base, release, variant, target).await
}

async fn install_from(
    http: &reqwest::Client,
    base: &str,
    release: &Release,
    variant: &str,
    target: &Path,
) -> Result<(), ProviderError> {
    let assets = release.assets(variant)?;
    let parent = target
        .parent()
        .ok_or_else(|| error("invalid build cache path"))?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let temp = Arc::new(
        tempfile::Builder::new()
            .prefix(".install-")
            .tempdir_in(parent)
            .map_err(io_error)?,
    );
    let stage = temp.path().join("build");
    fs::create_dir(&stage).map_err(io_error)?;
    for (name, digest) in assets {
        let url = format!("{base}/{name}");
        let archive = temp.path().join(name);
        download(http, &url, &archive, digest).await?;
        let dst = stage.clone();
        // Keep staging alive until the blocking unpack finishes, even if the caller cancels.
        let keep = Arc::clone(&temp);
        tokio::task::spawn_blocking(move || {
            let _keep = keep;
            extract(fs::File::open(&archive).map_err(io_error)?, &archive, &dst)
        })
        .await
        .map_err(|_| error("llama.cpp archive extraction failed"))??;
    }
    fs::write(
        stage.join(".complete"),
        format!("{} {variant}\n", release.build),
    )
    .map_err(io_error)?;
    match fs::rename(stage, target) {
        Ok(()) => Ok(()),
        Err(_) if target.join(".complete").is_file() => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    expected: &str,
) -> Result<(), ProviderError> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| error("cannot download the pinned llama.cpp release"))?;
    let mut response = response
        .error_for_status()
        .map_err(|e| error(format!("llama.cpp download returned {:?}", e.status())))?;
    let mut file = tokio::fs::File::create(path).await.map_err(io_error)?;
    let mut digest = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| error("llama.cpp release download interrupted"))?
    {
        digest.update(&chunk);
        file.write_all(&chunk).await.map_err(io_error)?;
    }
    file.flush().await.map_err(io_error)?;
    if !matches_digest(&digest.finalize(), expected) {
        return Err(error(
            "llama.cpp release SHA-256 does not match the pinned digest",
        ));
    }
    Ok(())
}

fn matches_digest(digest: &[u8], expected: &str) -> bool {
    expected.is_ascii()
        && expected.len() == digest.len() * 2
        && expected
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .zip(digest)
            .all(|(digits, byte)| {
                std::str::from_utf8(digits)
                    .ok()
                    .and_then(|s| u8::from_str_radix(s, 16).ok())
                    == Some(*byte)
            })
}

fn extract(file: fs::File, path: &Path, dest: &Path) -> Result<(), ProviderError> {
    if path.extension().is_some_and(|ext| ext == "zip") {
        let mut archive =
            zip::ZipArchive::new(file).map_err(|_| error("invalid llama.cpp ZIP archive"))?;
        // Windows release archives contain only directories and regular files.
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|_| error("invalid llama.cpp ZIP member"))?;
            let name: PathBuf = entry
                .enclosed_name()
                .ok_or_else(|| error("invalid release archive path"))?;
            if entry.is_symlink() {
                return Err(error("unexpected link in a Windows release"));
            }
            let member = dest.join(name);
            if entry.is_dir() {
                fs::create_dir_all(member).map_err(io_error)?;
            } else {
                if let Some(parent) = member.parent() {
                    fs::create_dir_all(parent).map_err(io_error)?;
                }
                io::copy(&mut entry, &mut fs::File::create(member).map_err(io_error)?)
                    .map_err(io_error)?;
            }
        }
        Ok(())
    } else {
        tar::Archive::new(GzDecoder::new(file))
            .unpack(dest)
            .map_err(io_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get};

    #[tokio::test]
    async fn downloads_are_verified_before_they_can_be_extracted() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let url = format!("http://{}/archive", listener.local_addr().unwrap());
        let service = Router::new().route("/archive", get(|| async { "abc" }));
        let serving = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
        let http = reqwest::Client::builder()
            .no_proxy()
            .tls_backend_preconfigured(roomkit_providers_http::tls_config().unwrap())
            .build()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("download");
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        download(&http, &url, &path, hash).await.unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"abc");
        let wrong = download(&http, &url, &path, &"0".repeat(64))
            .await
            .unwrap_err();
        assert!(wrong.message.contains("SHA-256"));
        serving.abort();
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use axum::{Router, routing::get};
    use flate2::{Compression, write::GzEncoder};
    use serde_json::json;
    use std::fmt::Write;

    fn archive() -> Vec<u8> {
        let encoder = GzEncoder::new(Vec::new(), Compression::fast());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "bin/llama-server", &b"test"[..])
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn reads_deflated_windows_release_members() {
        use std::io::Write as _;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("windows.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("bin/llama-server.exe", options).unwrap();
        writer.write_all(b"server").unwrap();
        drop(writer.finish().unwrap());
        extract(fs::File::open(&path).unwrap(), &path, root.path()).unwrap();
        assert_eq!(
            fs::read(root.path().join("bin/llama-server.exe")).unwrap(),
            b"server"
        );
    }

    #[tokio::test]
    async fn concurrent_installers_publish_a_complete_build_or_nothing() {
        let bytes = archive();
        let mut sha = String::new();
        for byte in Sha256::digest(&bytes) {
            write!(sha, "{byte:02x}").unwrap();
        }
        let release: Release = serde_json::from_value(json!({"build":"sample","variants":{
            "ok":[["build.tar.gz",sha]], "bad":[["build.tar.gz","00"]]
        }}))
        .unwrap();
        let server = Router::new().route("/build.tar.gz", get(move || async move { bytes }));
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let running = tokio::spawn(async { axum::serve(listener, server).await.unwrap() });
        let http = reqwest::Client::builder()
            .no_proxy()
            .tls_backend_preconfigured(roomkit_providers_http::tls_config().unwrap())
            .build()
            .unwrap();
        let cache = tempfile::tempdir().unwrap();
        let target = cache.path().join("installed");
        assert!(
            install_from(&http, &base, &release, "bad", &target)
                .await
                .is_err()
        );
        assert!(!target.exists());
        let first = install_from(&http, &base, &release, "ok", &target);
        let second = install_from(&http, &base, &release, "ok", &target);
        let (a, b) = tokio::join!(first, second);
        a.unwrap();
        b.unwrap();
        assert_eq!(fs::read(target.join("bin/llama-server")).unwrap(), b"test");
        assert_eq!(
            fs::read_to_string(target.join(".complete")).unwrap(),
            "sample ok\n"
        );
        assert_eq!(cache.path().read_dir().unwrap().count(), 1);
        running.abort();
    }
}
