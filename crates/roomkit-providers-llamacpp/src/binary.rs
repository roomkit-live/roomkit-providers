//! Locate a user-selected executable or a pinned build in RoomKit's cache.

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::LazyLock,
    time::Duration,
};

use roomkit_ai::ProviderError;
use serde::Deserialize;
use tokio::{process::Command, time::timeout};

use crate::{LlamaCppConfig, child_env::child_command, error, install};

#[derive(Deserialize)]
pub(crate) struct Release {
    pub(crate) build: String,
    variants: BTreeMap<String, Vec<(String, String)>>,
}

impl Release {
    pub(crate) fn assets(&self, variant: &str) -> Result<&[(String, String)], ProviderError> {
        self.variants
            .get(variant)
            .map(Vec::as_slice)
            .ok_or_else(|| error("unknown llama.cpp build variant"))
    }
}

pub(crate) fn release() -> Result<&'static Release, ProviderError> {
    static RELEASE: LazyLock<Result<Release, serde_json::Error>> =
        LazyLock::new(|| serde_json::from_str(include_str!("builds.json")));
    RELEASE
        .as_ref()
        .map_err(|_| error("invalid bundled llama.cpp release manifest"))
}

pub(crate) fn expand(path: &Path) -> PathBuf {
    if let (Ok(tail), Some(home)) = (path.strip_prefix("~"), env::home_dir()) {
        return home.join(tail);
    }
    path.to_owned()
}

pub(crate) struct Binary {
    pub(crate) executable: PathBuf,
    libraries: BTreeSet<PathBuf>,
}

impl Binary {
    pub(crate) fn command(&self, bundle: Option<&Path>) -> Result<Command, ProviderError> {
        let mut cmd = child_command(&self.executable);
        crate::loader::configure(&mut cmd, &self.libraries, bundle)?;
        Ok(cmd)
    }
}

pub(crate) async fn resolve(config: &LlamaCppConfig) -> Result<Binary, ProviderError> {
    if let Some(path) = &config.binary {
        let path = expand(path);
        let executable = if path.parent().is_some_and(|p| !p.as_os_str().is_empty()) {
            fs::canonicalize(path).map_err(io_error)?
        } else {
            on_path(&path)
                .ok_or_else(|| error("configured llama-server binary was not found on PATH"))?
        };
        return Ok(Binary {
            executable,
            libraries: BTreeSet::new(),
        });
    }
    let release = release()?;
    let variant = if let Some(chosen) = &config.variant {
        chosen.clone()
    } else {
        detect(release).await?
    };
    let root = if let Some(cache) = &config.cache_dir {
        expand(cache)
    } else {
        default_cache()?
    };
    let target = root.join(&release.build).join(&variant);
    if !target.join(".complete").is_file() {
        install::build(release, &variant, &target).await?;
    }
    tokio::task::spawn_blocking(move || locate(&target))
        .await
        .map_err(|_| error("cannot inspect the llama.cpp build"))?
}

fn default_cache() -> Result<PathBuf, ProviderError> {
    let base = env::var_os("XDG_CACHE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::home_dir().map(|p| p.join(".cache")))
        .ok_or_else(|| error("no user cache directory; set cache_dir"))?;
    Ok(base.join("roomkit").join("llama.cpp"))
}

fn locate(root: &Path) -> Result<Binary, ProviderError> {
    let mut dirs = vec![root.to_owned()];
    let mut libraries = BTreeSet::new();
    let mut servers = BTreeSet::new();
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            if entry.file_type().map_err(io_error)?.is_dir() {
                dirs.push(path);
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(name.as_ref(), "llama-server" | "llama-server.exe") {
                servers.insert(path);
            } else if name.ends_with(".dll")
                || name.ends_with(".dylib")
                || name.ends_with(".so")
                || name.contains(".so.")
            {
                libraries.insert(dir.clone());
            }
        }
    }
    let executable = servers
        .pop_first()
        .ok_or_else(|| error("cached llama.cpp build has no llama-server"))?;
    Ok(Binary {
        executable,
        libraries,
    })
}

fn on_path(name: &Path) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?).find_map(|dir| {
        let mut file = dir.join(name);
        if cfg!(windows) && file.extension().is_none() {
            file.set_extension("exe");
        }
        let meta = file.metadata().ok()?;
        if !meta.is_file() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o111 == 0 {
                return None;
            }
        }
        fs::canonicalize(file).ok()
    })
}

async fn detect(release: &Release) -> Result<String, ProviderError> {
    let arch = match env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return Err(error("no llama.cpp build for this CPU; set binary")),
    };
    let os = env::consts::OS;
    if os == "macos" {
        return Ok(format!("macos-{arch}"));
    }
    if !matches!(os, "linux" | "windows") {
        return Err(error("no llama.cpp build for this OS; set binary"));
    }
    let Some(smi) = on_path(Path::new("nvidia-smi")) else {
        return Ok(format!("{os}-{arch}-cpu"));
    };
    let mut command = child_command(smi);
    command.kill_on_drop(true).stdin(Stdio::null());
    let driver = match timeout(Duration::from_secs(10), command.output()).await {
        Ok(Ok(output)) => cuda_major(&String::from_utf8_lossy(&output.stdout)),
        _ => None,
    };
    for major in [13, 12] {
        let name = format!("{os}-{arch}-cuda-{major}");
        if driver.is_some_and(|n| n >= major) && release.variants.contains_key(&name) {
            return Ok(name);
        }
    }
    Ok(format!("{os}-{arch}-cpu"))
}

fn cuda_major(report: &str) -> Option<u32> {
    let (_, version) = report.split_once("CUDA Version:")?;
    let digits: String = version
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

pub(crate) fn io_error(e: io::Error) -> ProviderError {
    error(format!("llama.cpp: {e}")).with_source(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_the_driver_major_and_finds_both_build_and_runtime_libraries() {
        assert_eq!(cuda_major("CUDA Version: 13.5 |"), Some(13));
        assert_eq!(cuda_major("no GPU"), None);
        let temp = tempfile::tempdir().unwrap();
        let bin = temp.path().join("server");
        let cuda = temp.path().join("runtime");
        fs::create_dir(&bin).unwrap();
        fs::create_dir(&cuda).unwrap();
        fs::write(bin.join("llama-server"), []).unwrap();
        fs::write(bin.join("libllama.so.1"), []).unwrap();
        fs::write(cuda.join("cudart.dll"), []).unwrap();
        let found = locate(temp.path()).unwrap();
        assert_eq!(found.executable, bin.join("llama-server"));
        assert_eq!(found.libraries, BTreeSet::from([bin, cuda]));
    }
}
