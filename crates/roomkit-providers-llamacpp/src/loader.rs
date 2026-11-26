//! Set the child's native library search path, removing paths injected by a frozen host.

use std::{
    collections::BTreeSet,
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

use roomkit_ai::ProviderError;
use tokio::process::Command;

use crate::error;

pub(crate) fn configure(
    cmd: &mut Command,
    libraries: &BTreeSet<PathBuf>,
    bundle: Option<&Path>,
) -> Result<(), ProviderError> {
    let loader = if cfg!(windows) {
        "PATH"
    } else if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    for key in ["LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "PATH"] {
        let clean = bundle.filter(|_| key != "PATH");
        let prepend = key == loader && !libraries.is_empty();
        if clean.is_none() && !prepend {
            continue;
        }
        let mut value = env::var_os(key);
        if let Some(bundle) = clean {
            let original_key = format!("{key}_ORIG");
            cmd.env_remove(&original_key);
            value = restored(value.as_ref(), env::var_os(original_key), bundle)?;
        }
        if prepend {
            let inherited = value.iter().flat_map(env::split_paths);
            value = Some(
                env::join_paths(libraries.iter().cloned().chain(inherited))
                    .map_err(|_| error("cannot set the llama.cpp library path"))?,
            );
        }
        if let Some(value) = value {
            cmd.env(key, value);
        } else {
            cmd.env_remove(key);
        }
    }
    Ok(())
}

fn restored(
    current: Option<&OsString>,
    original: Option<OsString>,
    bundle: &Path,
) -> Result<Option<OsString>, ProviderError> {
    if original.is_some() {
        return Ok(original);
    }
    let bundle = std::path::absolute(bundle).map_err(crate::binary::io_error)?;
    let kept: Vec<_> = current
        .into_iter()
        .flat_map(env::split_paths)
        .filter(|path| !path.as_os_str().is_empty() && !path.starts_with(&bundle))
        .collect();
    if kept.is_empty() {
        return Ok(None);
    }
    env::join_paths(kept)
        .map(Some)
        .map_err(|_| error("cannot restore the host library path"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_the_frozen_hosts_original_libraries_only() {
        let bundle = env::temp_dir().join("bundle");
        let user = env::temp_dir().join("user");
        let paths = env::join_paths([bundle.join("lib"), user.clone()]).unwrap();
        assert_eq!(
            restored(Some(&paths), None, &bundle).unwrap(),
            Some(user.into_os_string())
        );
        assert_eq!(
            restored(Some(&paths), Some("original".into()), &bundle).unwrap(),
            Some("original".into())
        );
    }
}
