//! Core ML models on disk: fetched once, the first time they are needed,
//! from a fixed revision on Hugging Face and checked against the digests
//! here before they are used.
//!
//! Karaoke's is UVR's MDX-Net "Voc FT" by Kimberley Jensen, converted to
//! Core ML by gyoom (<https://huggingface.co/gyoom/UVR-MDX-CoreML>), MIT
//! licensed. Word-by-word lyrics' letter model is described in
//! `crate::words::coreml`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

/// A Core ML package on Hugging Face, pinned to a revision, with the
/// SHA-256 digest of each of its files.
pub struct Package {
    pub repository: &'static str,
    pub revision: &'static str,
    pub name: &'static str,
    pub files: &'static [(&'static str, &'static str)],
}

/// Karaoke's vocal model.
pub const VOCALS: Package = Package {
    repository: "https://huggingface.co/gyoom/UVR-MDX-CoreML/resolve",
    revision: "aa27ab16896fdb298cdb73cceb76c950e7580c00",
    name: "UVR-MDX-NET-Voc_FT.mlpackage",
    files: &[
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
    ],
};

/// Where the vocal model lives once fetched.
pub fn package(dir: &Path) -> PathBuf {
    VOCALS.path(dir)
}

pub fn installed(dir: &Path) -> bool {
    VOCALS.installed(dir)
}

/// Fetches and checks the vocal model, then moves it into place.
pub async fn fetch(http: &reqwest::Client, dir: &Path) -> Result<PathBuf> {
    VOCALS.fetch(http, dir).await
}

impl Package {
    /// Where the package lives once fetched. It is only ever moved here
    /// whole and checked, so its presence means it is good.
    pub fn path(&self, dir: &Path) -> PathBuf {
        dir.join(self.name)
    }

    pub fn installed(&self, dir: &Path) -> bool {
        self.files
            .first()
            .is_some_and(|(name, _)| self.path(dir).join(name).is_file())
    }

    /// Fetches and checks the package, then moves it into place. Files are
    /// written as they arrive, so a large one never sits in memory.
    pub async fn fetch(&self, http: &reqwest::Client, dir: &Path) -> Result<PathBuf> {
        let target = self.path(dir);
        if self.installed(dir) {
            return Ok(target);
        }
        let partial = dir.join(format!("{}.partial", self.name));
        if partial.exists() {
            tokio::fs::remove_dir_all(&partial).await.ok();
        }
        for (name, digest) in self.files {
            let url = format!("{}/{}/{}/{name}", self.repository, self.revision, self.name);
            let mut response = http
                .get(&url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .with_context(|| format!("downloading {name}"))?;
            let path = partial.join(name);
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let mut file = tokio::fs::File::create(&path).await?;
            let mut hash = Sha256::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .with_context(|| format!("downloading {name}"))?
            {
                hash.update(&chunk);
                file.write_all(&chunk).await?;
            }
            file.flush().await?;
            if hex(&hash.finalize()) != *digest {
                bail!("{name} is not the expected file");
            }
        }
        // Moved into place whole, so a package that is there is complete.
        tokio::fs::rename(&partial, &target)
            .await
            .with_context(|| format!("moving {} into place", self.name))?;
        Ok(target)
    }
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
