//! The check of an ID token: its signature by one of the issuer's
//! keys, and what it claims.

use anyhow::{Context, Result, anyhow, ensure};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::signature::{
    RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents,
};
use serde_json::Value;

/// Seconds a clock may be off.
const SKEW: u64 = 30;

/// The account an ID token is of.
pub(super) struct Claims {
    pub sub: String,
    pub email: String,
}

fn decode(part: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(part)
        .context("the ID token is not written in base64url")
}

/// Checks `token`, signed with RS256 by a key of `keys` (a JWKS), for
/// the client `audience` and the sign-in of `nonce`.
pub(super) fn verify(
    token: &str,
    keys: &Value,
    issuer: &str,
    audience: &str,
    nonce: &str,
    now: u64,
) -> Result<Claims> {
    let mut parts = token.split('.');
    let (Some(head), Some(body), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(anyhow!("the ID token is not a JWT"));
    };
    let header = serde_json::from_slice::<Value>(&decode(head)?)?;
    ensure!(
        header["alg"] == "RS256",
        "the ID token is signed with {}, not RS256",
        header["alg"]
    );
    let kid = header["kid"].as_str();
    let key = keys["keys"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|key| {
            key["kty"] == "RSA"
                && (kid.is_none() || key["kid"].as_str() == kid)
        })
        .context("no key of the issuer signed the ID token")?;
    let part =
        |name: &str| decode(key[name].as_str().unwrap_or_default());
    RsaPublicKeyComponents {
        n: part("n")?,
        e: part("e")?,
    }
    .verify(
        &RSA_PKCS1_2048_8192_SHA256,
        format!("{head}.{body}").as_bytes(),
        &decode(signature)?,
    )
    .map_err(|_| anyhow!("the ID token's signature does not hold"))?;
    let said = serde_json::from_slice::<Value>(&decode(body)?)?;
    claims(&said, issuer, audience, nonce, now)
}

/// The account in the claims of an ID token whose signature holds,
/// once they are found to be for this sign-in.
fn claims(
    said: &Value,
    issuer: &str,
    audience: &str,
    nonce: &str,
    now: u64,
) -> Result<Claims> {
    ensure!(
        said["iss"] == issuer,
        "the ID token is of another issuer"
    );
    let ours = match &said["aud"] {
        Value::String(one) => one == audience,
        Value::Array(all) => all.iter().any(|one| one == audience),
        _ => false,
    };
    ensure!(ours, "the ID token is for another client");
    ensure!(
        said["exp"].as_u64().is_some_and(|exp| exp + SKEW > now),
        "the ID token has expired"
    );
    ensure!(
        said["nonce"] == nonce,
        "the ID token is of another sign-in"
    );
    let sub = said["sub"]
        .as_str()
        .filter(|sub| !sub.is_empty())
        .context("the ID token names no account")?;
    Ok(Claims {
        sub: sub.into(),
        email: said["email"].as_str().unwrap_or_default().into(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn an_id_token_holds_only_for_its_issuer_client_nonce_and_time() {
        let good = json!({"iss": "https://auth.example", "aud": ["oaiapp_1"],
            "exp": 1000, "nonce": "n", "sub": "user-1", "email": "a@b.c"});
        let check = |said: &Value, now| {
            claims(said, "https://auth.example", "oaiapp_1", "n", now)
        };
        let account = check(&good, 990).unwrap();
        assert_eq!(
            (&*account.sub, &*account.email),
            ("user-1", "a@b.c")
        );
        assert!(check(&good, 2000).is_err());
        for (key, wrong) in [
            ("iss", json!("https://other.example")),
            ("aud", json!("oaiapp_2")),
            ("nonce", json!("m")),
            ("sub", json!("")),
        ] {
            let mut bad = good.clone();
            bad[key] = wrong;
            assert!(check(&bad, 990).is_err(), "{key}");
        }
    }
}
