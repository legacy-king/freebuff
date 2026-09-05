use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,        // user id
    pub email: String,
    pub exp: i64,
    pub iat: i64,
    pub role: String,
    pub org_id: Option<String>,
}

impl Claims {
    pub fn user_id(&self) -> Uuid {
        Uuid::parse_str(&self.sub).unwrap_or_default()
    }

    pub fn org_id(&self) -> Option<Uuid> {
        self.org_id.as_ref().and_then(|s| Uuid::parse_str(s).ok())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyClaims {
    pub sub: String,        // api key id
    pub project_id: String,
    pub key_type: String,
    pub scopes: Vec<String>,
    pub exp: Option<i64>,
    pub iat: i64,
}

impl ApiKeyClaims {
    pub fn project_id(&self) -> Uuid {
        Uuid::parse_str(&self.project_id).unwrap_or_default()
    }
}

pub fn create_access_token(
    user_id: Uuid,
    email: &str,
    secret: &str,
    expiration_secs: i64,
    org_id: Option<Uuid>,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        iat: now.timestamp(),
        exp: (now + Duration::seconds(expiration_secs)).timestamp(),
        role: "authenticated".into(),
        org_id: org_id.map(|id| id.to_string()),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn decode_access_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

pub fn create_api_key_token(
    key_id: Uuid,
    project_id: Uuid,
    key_type: &str,
    scopes: Vec<String>,
    secret: &str,
    expires_in_secs: Option<i64>,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let claims = ApiKeyClaims {
        sub: key_id.to_string(),
        project_id: project_id.to_string(),
        key_type: key_type.to_string(),
        scopes,
        iat: now.timestamp(),
        exp: expires_in_secs.map(|secs| (now + Duration::seconds(secs)).timestamp()),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn decode_api_key_token(
    token: &str,
    secret: &str,
) -> Result<ApiKeyClaims, jsonwebtoken::errors::Error> {
    let token_data = decode::<ApiKeyClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}
