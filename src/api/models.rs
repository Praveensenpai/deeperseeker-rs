use crate::domain::openai::{ModelList, ModelObject};
use axum::{response::IntoResponse, Json};

const KNOWN_MODELS: &[(&str, &str)] = &[
    ("v4.1flash", "deepseek"),
    ("anthropic/claude-v4.1flash", "anthropic"),
];

fn make_model(id: &'static str, owner: &'static str) -> ModelObject {
    ModelObject {
        id: id.to_string(),
        object: "model".to_string(),
        created: 1700000000,
        owned_by: owner.to_string(),
    }
}

pub async fn list_models() -> impl IntoResponse {
    let models = KNOWN_MODELS
        .iter()
        .map(|&(id, owner)| make_model(id, owner))
        .collect();

    Json(ModelList {
        object: "list".to_string(),
        data: models,
    })
}
