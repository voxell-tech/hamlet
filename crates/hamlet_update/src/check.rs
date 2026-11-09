//! Finding a newer release in its signed manifest, and downloading
//! the file this copy's kind of install takes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hamlet_protocol::Version;
use reqwest::Client;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::{Install, KEY};

/// The latest release's manifest. Its signature is beside it, at the
/// same address with `.sig` added.
pub const FEED: &str = "https://github.com/voxell-tech/hamlet/releases/latest/download/latest.json";

/// The most a manifest or its signature may weigh.
const MOST: usize = 1 << 20;

/// A release's manifest, `latest.json`.
#[derive(Deserialize)]
struct Manifest {
    version: Version,
    #[serde(default)]
    notes: String,
    /// The release's page.
    page: String,
    /// Each kind of install's file, by [`Install::file`].
    files: BTreeMap<String, Asset>,
}

/// A file of a release.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub url: String,
    /// Its SHA-256, in hex.
    pub sha256: String,
    /// Its length in bytes.
    pub size: u64,
}

/// A release newer than this copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub version: Version,
    pub notes: String,
    pub page: String,
    /// The file this kind of install takes, if the release has one.
    pub asset: Option<Asset>,
}

/// Where releases are looked for, and the key their manifests are
/// signed with.
pub struct Feed {
    url: String,
    key: Vec<u8>,
}

/// A client for the feed and its files.
pub fn client() -> reqwest::Result<Client> {
    Client::builder()
        .user_agent(concat!(
            "Hamlet/",
            env!("CARGO_PKG_VERSION"),
            " (https://hamlet.gg)"
        ))
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .build()
}

impl Feed {
    /// The manifest at `url`, signed with the base64 `key`.
    pub fn new(url: String, key: &str) -> Result<Self> {
        let key = STANDARD
            .decode(key.trim())
            .context("The update key is not base64")?;
        ensure!(
            key.len() == 32,
            "The update key is not an Ed25519 key"
        );
        Ok(Self { url, key })
    }

    /// This copy's feed: `HAMLET_UPDATE_FEED`, else [`FEED`]. None
    /// when it does not update.
    pub fn here() -> Option<Self> {
        if !crate::is_enabled() {
            return None;
        }
        let url = std::env::var("HAMLET_UPDATE_FEED")
            .ok()
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| FEED.into());
        Self::new(url, KEY?).ok()
    }

    /// The release in the manifest, when its signature holds and it
    /// is newer than `than`.
    pub async fn check(
        &self,
        client: &Client,
        install: &Install,
        than: Version,
    ) -> Result<Option<Found>> {
        let manifest = fetch(client, &self.url).await?;
        let signature =
            fetch(client, &format!("{}.sig", self.url)).await?;
        let signature = STANDARD
            .decode(signature.trim_ascii())
            .context("The manifest's signature is not base64")?;
        UnparsedPublicKey::new(&ED25519, &self.key)
            .verify(&manifest, &signature)
            .ok()
            .context("The manifest's signature does not hold")?;
        let manifest = serde_json::from_slice::<Manifest>(&manifest)
            .context("The manifest is not one Hamlet reads")?;
        if manifest.version <= than {
            return Ok(None);
        }
        let asset = install
            .file()
            .and_then(|file| manifest.files.get(file).cloned());
        Ok(Some(Found {
            version: manifest.version,
            notes: manifest.notes,
            page: manifest.page,
            asset,
        }))
    }
}

/// The body at `url`, refused past [`MOST`] bytes.
async fn fetch(client: &Client, url: &str) -> Result<Vec<u8>> {
    let mut answer = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("Cannot fetch {url}"))?;
    let mut bytes = Vec::new();
    while let Some(chunk) = answer.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= MOST,
            "{url} is too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// The file's name at the end of `url`, when it is one that names no
/// other folder.
fn name_of(url: &str) -> Option<&str> {
    let name = url.rsplit('/').next()?;
    let is_plain =
        |c: char| c.is_ascii_alphanumeric() || "._-+~".contains(c);
    (!name.starts_with('.') && name.chars().all(is_plain))
        .then_some(name)
}

/// The SHA-256 of the file at `path`, in hex.
async fn sum(path: &Path) -> Option<String> {
    let bytes = tokio::fs::read(path).await.ok()?;
    let digest =
        tokio::task::spawn_blocking(move || Sha256::digest(bytes))
            .await
            .ok()?;
    Some(hex(&digest))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `asset`, downloaded into `folder` and checked against its sum and
/// length. One downloaded already is kept.
pub async fn download(
    client: &Client,
    asset: &Asset,
    folder: &Path,
) -> Result<PathBuf> {
    let name = name_of(&asset.url)
        .context("The update's file has no plain name")?;
    let path = folder.join(name);
    let want = asset.sha256.to_ascii_lowercase();
    if sum(&path).await.as_ref() == Some(&want) {
        return Ok(path);
    }
    tokio::fs::create_dir_all(folder).await?;
    let part = folder.join(format!("{name}.part"));
    let fetched = fetch_into(client, asset, &part).await;
    let fetched = match fetched {
        Ok(got) if got == want => Ok(()),
        Ok(_) => Err(anyhow::anyhow!(
            "{name} is not the file the release names"
        )),
        Err(error) => Err(error),
    };
    if let Err(error) = fetched {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(error);
    }
    tokio::fs::rename(&part, &path).await?;
    Ok(path)
}

/// Writes `asset` to `part`, no longer than it says it is, and gives
/// its sum.
async fn fetch_into(
    client: &Client,
    asset: &Asset,
    part: &Path,
) -> Result<String> {
    let mut answer = client
        .get(&asset.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .with_context(|| format!("Cannot download {}", asset.url))?;
    let mut file = tokio::fs::File::create(part).await?;
    let (mut hasher, mut length) = (Sha256::new(), 0u64);
    while let Some(chunk) = answer.chunk().await? {
        length += chunk.len() as u64;
        if length > asset.size {
            bail!("{} is longer than the release says", asset.url);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    ensure!(length == asset.size, "{} ended early", asset.url);
    Ok(hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    use super::*;

    /// A loopback server of `files` by path, and its address.
    async fn serve(files: Vec<(String, Vec<u8>)>) -> String {
        let listener =
            TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address =
            format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let (mut stream, _) =
                    listener.accept().await.unwrap();
                let mut head = vec![0; 4096];
                let read = stream.read(&mut head).await.unwrap();
                let head = String::from_utf8_lossy(&head[..read]);
                let path = head.split(' ').nth(1).unwrap_or_default();
                let reply = match files.iter().find(|(at, _)| at == path) {
                    Some((_, body)) => {
                        let mut reply = format!(
                            "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        reply.extend_from_slice(body);
                        reply
                    }
                    None => b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec(),
                };
                stream.write_all(&reply).await.unwrap();
            }
        });
        address
    }

    fn keys() -> (Ed25519KeyPair, String) {
        let pkcs8 =
            Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
                .unwrap();
        let pair =
            Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public = STANDARD.encode(pair.public_key().as_ref());
        (pair, public)
    }

    fn manifest(version: &str, deb: &Asset) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "version": version,
            "notes": "Roads",
            "page": "https://example.com/v",
            "files": { "deb": {
                "url": deb.url, "sha256": deb.sha256, "size": deb.size,
            } },
        }))
        .unwrap()
    }

    fn asset(url: &str, body: &[u8]) -> Asset {
        Asset {
            url: url.into(),
            sha256: hex(&Sha256::digest(body)),
            size: body.len() as u64,
        }
    }

    #[tokio::test]
    async fn only_a_newer_release_whose_signature_holds_is_found() {
        let (pair, public) = keys();
        let deb = asset(
            "https://example.com/hamlet_0.2.0_amd64.deb",
            b"deb",
        );
        let signed = manifest("0.2.0", &deb);
        let signature = STANDARD.encode(pair.sign(&signed).as_ref());
        let mut forged = signed.clone();
        forged.extend_from_slice(b" ");
        let address = serve(vec![
            ("/latest.json".into(), signed),
            (
                "/latest.json.sig".into(),
                signature.clone().into_bytes(),
            ),
            ("/forged.json".into(), forged),
            ("/forged.json.sig".into(), signature.into_bytes()),
        ])
        .await;
        let client = client().unwrap();
        let feed =
            Feed::new(format!("{address}/latest.json"), &public)
                .unwrap();
        let found = feed
            .check(&client, &Install::Deb, Version::new(0, 1, 0))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.version, Version::new(0, 2, 0));
        assert_eq!(found.asset, Some(deb));
        let none = feed
            .check(&client, &Install::Manual, Version::new(0, 1, 0))
            .await;
        assert_eq!(none.unwrap().unwrap().asset, None);
        let same = feed
            .check(&client, &Install::Deb, Version::new(0, 2, 0))
            .await;
        assert_eq!(same.unwrap(), None);
        let forged =
            Feed::new(format!("{address}/forged.json"), &public)
                .unwrap();
        assert!(
            forged
                .check(&client, &Install::Deb, Version::new(0, 1, 0))
                .await
                .is_err()
        );
        let (_, other) = keys();
        let stranger =
            Feed::new(format!("{address}/latest.json"), &other)
                .unwrap();
        assert!(
            stranger
                .check(&client, &Install::Deb, Version::new(0, 1, 0))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_download_is_kept_only_when_its_sum_and_length_match() {
        let body = b"the new hamlet".to_vec();
        let address =
            serve(vec![("/hamlet.deb".into(), body.clone())]).await;
        let folder = std::env::temp_dir()
            .join(format!("hamlet-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let client = client().unwrap();
        let good = asset(&format!("{address}/hamlet.deb"), &body);
        let path = download(&client, &good, &folder).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), body);
        std::fs::remove_file(&path).unwrap();
        let wrong = Asset {
            sha256: hex(&Sha256::digest(b"other")),
            ..good.clone()
        };
        assert!(download(&client, &wrong, &folder).await.is_err());
        let short = Asset {
            size: good.size - 1,
            ..good
        };
        assert!(download(&client, &short, &folder).await.is_err());
        let left = std::fs::read_dir(&folder).unwrap().count();
        assert_eq!(left, 0, "nothing is left of a refused download");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
