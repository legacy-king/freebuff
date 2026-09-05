use sha2::{Sha256, Digest};
use uuid::Uuid;

use crate::AppState;
use freebuff_shared::{AppError, CreateApiKey, CreateApiKeyResponse, ApiKeyType};

pub async fn create_api_key(
    state: &AppState,
    project_id: Uuid,
    input: CreateApiKey,
) -> Result<CreateApiKeyResponse, AppError> {
    let key_id = Uuid::new_v4();
    let key_prefix = format!("fb_{}", &key_id.to_string()[..8]);

    // Generate a full key
    let full_key = format!("{}.{}", key_prefix, hex::encode(rand::random::<[u8; 32]>()));

    // Hash the full key for storage
    let mut hasher = Sha256::new();
    hasher.update(full_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    let scopes = input.scopes.unwrap_or_default();

    let key = crate::db::api_keys::create_key(
        &state.db,
        project_id,
        &input.name,
        &key_hash,
        &key_prefix,
        input.key_type.clone(),
        &scopes,
        input.expires_at,
    )
    .await?;

    Ok(CreateApiKeyResponse {
        id: key.id,
        name: key.name,
        key: full_key,
        key_prefix: key.key_prefix,
        key_type: key.key_type,
        scopes: key.scopes,
        expires_at: key.expires_at,
        created_at: key.created_at,
    })
}
