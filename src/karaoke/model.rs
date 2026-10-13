//! Karaoke's vocal model on disk: fetched once, the first time karaoke is
//! turned on, from a fixed revision on Hugging Face and checked against the
//! digests here before it is used.
//!
//! The model is UVR's MDX-Net "Voc FT" by Kimberley Jensen, converted to
//! Core ML by gyoom (<https://huggingface.co/gyoom/UVR-MDX-CoreML>), MIT
//! licensed.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

const REPOSITORY: &str = "https://huggingface.co/gyoom/UVR-MDX-CoreML/resolve";
const REVISION: &str = "aa27ab16896fdb298cdb73cceb76c950e7580c00";
const PACKAGE: &str = "UVR-MDX-NET-Voc_FT.mlpackage";

/// The package's files and their SHA-256 digests.
const FILES: [(&str, &str); 3] = [
    (
        "Manifest.json",
        "966971d49a91cbd9d43d15e40ca61e95aac72fcfe0e0acd3ece5df9818680b34",
    ),
    (
        "Data/com.apple.CoreML/model.mlmodel",
        "bdd6a5c0b2d3d9eae431f82a58e194c43f08cc3f9da664fdb4daa9d7c4b9bc43",
    ),
    (
        "Data/com.apple.CoreML/weights/weight.bin",
        "ee8aeba2251618d8248bfc3bbad49bb5d3b3ee0b6caa47182e08f2e5e3131c22",
    ),
];

/// Where the model lives once fetched. It is only ever moved here whole and
/// checked, so its presence means it is good.
pub fn package(dir: &Path) -> PathBuf {
    dir.join(PACKAGE)
}

pub fn installed(dir: &Path) -> bool {
    package(dir).join(FILES[0].0).is_file()
}

/// Fetches and checks the model, then moves it into place.
pub async fn fetch(http: &reqwest::Client, dir: &Path) -> Result<PathBuf> {
    let target = package(dir);
    if installed(dir) {
        return Ok(target);
    }
    let partial = dir.join(format!("{PACKAGE}.partial"));
    if partial.exists() {
        tokio::fs::remove_dir_all(&partial).await.ok();
    }
    for (name, digest) in FILES {
        let url = format!("{REPOSITORY}/{REVISION}/{PACKAGE}/{name}");
        let response = http
            .get(&url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .with_context(|| format!("downloading {name}"))?;
        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("downloading {name}"))?;
        let found = hex(&Sha256::digest(&bytes));
        if found != digest {
            bail!("{name} is not the expected file");
        }
        let path = partial.join(name);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, &bytes).await?;
    }
    // Moved into place whole, so a package that is there is complete.
    tokio::fs::rename(&partial, &target)
        .await
        .context("moving the vocal model into place")?;
    Ok(target)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fetches the real model into `KARAOKE_FETCH_DIR`, checking the
    /// address and the digests here against what Hugging Face serves.
    #[tokio::test]
    #[ignore = "downloads 33 MB"]
    async fn the_model_is_fetched_and_checked() {
        let dir = PathBuf::from(std::env::var("KARAOKE_FETCH_DIR").unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        let path = fetch(&reqwest::Client::new(), &dir).await.unwrap();
        assert!(installed(&dir));
        assert_eq!(path, package(&dir));
        // A second call finds it there.
        assert_eq!(fetch(&reqwest::Client::new(), &dir).await.unwrap(), path);
    }
}
