use crate::domain::openai::{ModelList, ModelObject};
use axum::{response::IntoResponse, Json};

pub async fn list_models() -> impl IntoResponse {
    let models = vec![
        ModelObject {
            id: "v4.1flash".to_string(),
            object: "model".to_string(),
            created: 1700000000,
            owned_by: "deepseek".to_string(),
        },
        ModelObject {
            id: "anthropic/claude-v4.1flash".to_string(),
            object: "model".to_string(),
            created: 1700000000,
            owned_by: "anthropic".to_string(),
        },
    ];

    Json(ModelList {
        object: "list".to_string(),
        data: models,
    })
}
