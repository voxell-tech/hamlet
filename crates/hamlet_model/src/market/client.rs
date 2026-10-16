//! The bazaar's server, hamlet-server, as it is called: every route
//! of `hamlet-server/docs/api.md` that Hamlet uses.

use std::fmt;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use super::lot::{
    Approved, Bought, Detail, Device, Draft, Kind, Lot, Owned, Page,
    Paying, PurchaseState, Release, Said, User,
};
use super::package::Package;
use crate::wardrobe::Asset;

/// The wait for any one answer. A package may be ten megabytes.
const PATIENCE: Duration = Duration::from_secs(120);

/// The most listings asked for at once.
const PAGE: u32 = 100;

/// The most pages of one kind asked for.
const PAGES: u32 = 10;

/// The bazaar's server, called as the one signed in, if anyone is.
#[derive(Clone, Debug)]
pub struct Client {
    base: String,
    token: Option<String>,
    http: reqwest::Client,
}

/// A refusal from the server: what it said, and its status.
#[derive(Clone, Debug, PartialEq)]
pub struct Refused {
    pub status: u16,
    /// The server's code for it, such as `pending` or `conflict`.
    pub code: String,
    pub message: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refused {}

/// The code of the server's refusal behind `error`, if one is.
pub fn refused(error: &anyhow::Error) -> Option<&str> {
    error.downcast_ref::<Refused>().map(|r| r.code.as_str())
}

#[derive(Deserialize)]
struct Fault {
    error: FaultBody,
}

#[derive(Deserialize)]
struct FaultBody {
    code: String,
    message: String,
}

/// What came of a sign-in asked after.
#[derive(Clone, Debug, PartialEq)]
pub enum Poll {
    /// Not approved yet.
    Pending,
    /// Asked too often: wait five seconds more each time.
    SlowDown,
    Approved(Approved),
}

impl Client {
    pub fn new(base: &str, token: Option<String>) -> Client {
        Client {
            base: base.trim_end_matches('/').to_string(),
            token,
            http: reqwest::Client::builder()
                .timeout(PATIENCE)
                .build()
                .unwrap_or_default(),
        }
    }

    /// `path` on the server, whole.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn ask(&self, method: Method, path: &str) -> RequestBuilder {
        let asked = self.http.request(method, self.url(path));
        match &self.token {
            Some(token) => asked.bearer_auth(token),
            None => asked,
        }
    }

    async fn send(&self, asked: RequestBuilder) -> Result<Response> {
        let answer = asked.send().await.with_context(|| {
            format!("the bazaar at {} did not answer", self.base)
        })?;
        if answer.status().is_success() {
            return Ok(answer);
        }
        let status = answer.status();
        let text = answer.text().await.unwrap_or_default();
        let refusal = match serde_json::from_str::<Fault>(&text) {
            Ok(fault) => Refused {
                status: status.as_u16(),
                code: fault.error.code,
                message: fault.error.message,
            },
            Err(_) => Refused {
                status: status.as_u16(),
                code: "internal".into(),
                message: format!("the bazaar answered {status}"),
            },
        };
        Err(refusal.into())
    }

    async fn json<T: DeserializeOwned>(
        &self,
        asked: RequestBuilder,
    ) -> Result<T> {
        let answer = self.send(asked).await?;
        answer
            .json::<T>()
            .await
            .context("the bazaar's answer could not be read")
    }

    /// Begins a sign-in with GitHub.
    pub async fn start_sign_in(&self) -> Result<Device> {
        self.json(self.ask(Method::POST, "/v1/auth/device")).await
    }

    /// Asks whether the sign-in of `device_code` was approved.
    pub async fn poll_sign_in(
        &self,
        device_code: &str,
    ) -> Result<Poll> {
        let asked = self
            .ask(Method::POST, "/v1/auth/device/poll")
            .json(&json!({ "deviceCode": device_code }));
        match self.json::<Approved>(asked).await {
            Ok(approved) => Ok(Poll::Approved(approved)),
            Err(e) => match refused(&e) {
                Some("pending") => Ok(Poll::Pending),
                Some("slow_down") => Ok(Poll::SlowDown),
                _ => Err(e),
            },
        }
    }

    pub async fn sign_out(&self) -> Result<()> {
        self.send(self.ask(Method::DELETE, "/v1/auth/session"))
            .await
            .map(drop)
    }

    pub async fn me(&self) -> Result<User> {
        self.json(self.ask(Method::GET, "/v1/me")).await
    }

    /// The page on Stripe where the one signed in sets up how they
    /// are paid.
    pub async fn payouts(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct Link {
            url: String,
        }
        let link = self
            .json::<Link>(self.ask(Method::POST, "/v1/me/payouts"))
            .await?;
        Ok(link.url)
    }

    /// The listings of the one signed in, in every state.
    pub async fn mine(&self) -> Result<Vec<Lot>> {
        let page = self
            .json::<Page<Lot>>(
                self.ask(Method::GET, "/v1/me/listings"),
            )
            .await?;
        Ok(page.items)
    }

    /// Everything the one signed in owns.
    pub async fn library(&self) -> Result<Vec<Owned>> {
        let page = self
            .json::<Page<Owned>>(
                self.ask(Method::GET, "/v1/me/library"),
            )
            .await?;
        Ok(page.items)
    }

    /// Every listing of `kind` on the shelves.
    pub async fn lots(&self, kind: Kind) -> Result<Vec<Lot>> {
        let mut all = Vec::new();
        for at in 0..PAGES {
            let asked =
                self.ask(Method::GET, "/v1/listings").query(&[
                    ("kind", kind.key().to_string()),
                    ("limit", PAGE.to_string()),
                    ("offset", (at * PAGE).to_string()),
                ]);
            let page = self.json::<Page<Lot>>(asked).await?;
            let last = page.items.len() < PAGE as usize;
            all.extend(page.items);
            if last
                || page.total.is_some_and(|t| all.len() >= t as usize)
            {
                break;
            }
        }
        Ok(all)
    }

    pub async fn detail(&self, id: &str) -> Result<Detail> {
        self.json(
            self.ask(Method::GET, &format!("/v1/listings/{id}")),
        )
        .await
    }

    /// Puts up a listing, not yet on the shelves.
    pub async fn create(&self, draft: &Draft) -> Result<Lot> {
        self.json(self.ask(Method::POST, "/v1/listings").json(draft))
            .await
    }

    /// Changes a listing's words, price or tags.
    pub async fn change(
        &self,
        id: &str,
        draft: &Draft,
    ) -> Result<Lot> {
        let draft = Draft {
            kind: None,
            ..draft.clone()
        };
        let path = format!("/v1/listings/{id}");
        self.json(self.ask(Method::PATCH, &path).json(&draft)).await
    }

    /// Sets a listing's cover, a PNG, JPEG or WebP picture.
    pub async fn cover(
        &self,
        id: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<()> {
        let path = format!("/v1/listings/{id}/cover");
        let asked = self
            .ask(Method::PUT, &path)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(bytes);
        self.send(asked).await.map(drop)
    }

    /// Uploads a new version of a listing.
    pub async fn release(
        &self,
        id: &str,
        version: &str,
        changes: &str,
        package: &Package,
    ) -> Result<Release> {
        let path = format!("/v1/listings/{id}/versions");
        let body = json!({
            "version": version,
            "changes": changes,
            "package": package,
        });
        self.json(self.ask(Method::POST, &path).json(&body)).await
    }

    /// Puts a listing on the shelves.
    pub async fn publish(&self, id: &str) -> Result<Lot> {
        let path = format!("/v1/listings/{id}/publish");
        self.json(self.ask(Method::POST, &path)).await
    }

    /// Takes a listing off the shelves. Its owners keep it.
    pub async fn unlist(&self, id: &str) -> Result<Lot> {
        let path = format!("/v1/listings/{id}/unlist");
        self.json(self.ask(Method::POST, &path)).await
    }

    /// The package of `version` of a listing the one signed in owns
    /// or sells.
    pub async fn package(
        &self,
        id: &str,
        version: &str,
    ) -> Result<Package> {
        let path =
            format!("/v1/listings/{id}/versions/{version}/package");
        let answer = self.send(self.ask(Method::GET, &path)).await?;
        let bytes = answer.bytes().await?;
        Package::read(&bytes)
            .context("the package the bazaar sent could not be read")
    }

    /// Buys a listing: owned at once when it is free, or a page on
    /// Stripe to pay on.
    pub async fn buy(&self, id: &str) -> Result<Bought> {
        let path = format!("/v1/listings/{id}/purchase");
        self.json(self.ask(Method::POST, &path)).await
    }

    pub async fn paying(&self, purchase: &str) -> Result<Paying> {
        let path = format!("/v1/purchases/{purchase}");
        let state = self
            .json::<PurchaseState>(self.ask(Method::GET, &path))
            .await?;
        Ok(state.status)
    }

    /// Reviews a listing the one signed in owns.
    pub async fn review(
        &self,
        id: &str,
        stars: u8,
        words: &str,
    ) -> Result<Said> {
        let path = format!("/v1/listings/{id}/reviews");
        let body = json!({ "stars": stars, "words": words });
        self.json(self.ask(Method::POST, &path).json(&body)).await
    }

    /// Every official asset on the shelves, `owned` set for the one
    /// signed in.
    pub async fn assets(&self) -> Result<Vec<Asset>> {
        let page = self
            .json::<Page<Asset>>(self.ask(Method::GET, "/v1/assets"))
            .await?;
        Ok(page.items)
    }

    /// Every official asset the one signed in owns.
    pub async fn my_assets(&self) -> Result<Vec<Asset>> {
        let page = self
            .json::<Page<Asset>>(
                self.ask(Method::GET, "/v1/me/assets"),
            )
            .await?;
        Ok(page.items)
    }

    /// Buys the asset `id`: owned at once when it is free, or a page
    /// on Stripe to pay on.
    pub async fn buy_asset(&self, id: &str) -> Result<Bought> {
        let path = format!("/v1/assets/{id}/purchase");
        self.json(self.ask(Method::POST, &path)).await
    }

    pub async fn asset_paying(
        &self,
        purchase: &str,
    ) -> Result<Paying> {
        let path = format!("/v1/asset-purchases/{purchase}");
        let state = self
            .json::<PurchaseState>(self.ask(Method::GET, &path))
            .await?;
        Ok(state.status)
    }

    /// The file of the asset `id`, which the one signed in owns. It
    /// is kept in memory only.
    pub async fn asset_file(&self, id: &str) -> Result<Vec<u8>> {
        let path = format!("/v1/assets/{id}/file");
        let answer = self.send(self.ask(Method::GET, &path)).await?;
        Ok(answer.bytes().await?.to_vec())
    }

    /// The bytes of a picture on the server, by its path.
    pub async fn picture(&self, path: &str) -> Result<Vec<u8>> {
        let answer = self.send(self.ask(Method::GET, path)).await?;
        Ok(answer.bytes().await?.to_vec())
    }
}

/// True when `error` is the server saying the one signed in is not.
pub fn signed_out(error: &anyhow::Error) -> bool {
    error.downcast_ref::<Refused>().is_some_and(|r| {
        r.status == StatusCode::UNAUTHORIZED.as_u16()
    })
}
