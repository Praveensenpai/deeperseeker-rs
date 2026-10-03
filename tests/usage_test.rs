use deeperseeker::domain::usage::format_metric;
use deeperseeker::infra::db::{init_db, open_db};
use deeperseeker::infra::usage_db::{
    get_all_summaries, get_daily_breakdown, get_model_breakdown, record_usage,
};

#[test]
fn test_metric_formatting() {
    assert_eq!(format_metric(500, false), "500");
    assert_eq!(format_metric(1_500, false), "1.5K");
    assert_eq!(format_metric(45_210, false), "45.2K");
    assert_eq!(format_metric(2_140_000, false), "2.14M");
    assert_eq!(format_metric(1_050_000_000, false), "1.05B");
    assert_eq!(format_metric(2_140_500, true), "2140500");
}

#[tokio::test]
async fn test_db_usage_recording_and_summaries() {
    let conn = open_db(":memory:").await.unwrap();
    init_db(&conn).await.unwrap();

    // Record usage
    record_usage(&conn, "deepseek-chat", 100, 200, Some(1))
        .await
        .unwrap();
    record_usage(&conn, "deepseek-chat", 400, 600, Some(1))
        .await
        .unwrap();
    record_usage(&conn, "deepseek-reasoner", 1000, 2000, Some(2))
        .await
        .unwrap();

    let summaries = get_all_summaries(&conn).await.unwrap();
    assert_eq!(summaries.len(), 6);

    let today = summaries.iter().find(|s| s.period == "Today").unwrap();
    assert_eq!(today.requests, 3);
    assert_eq!(today.prompt_tokens, 1500);
    assert_eq!(today.completion_tokens, 2800);
    assert_eq!(today.total_tokens, 4300);

    let all_time = summaries.iter().find(|s| s.period == "All Time").unwrap();
    assert_eq!(all_time.requests, 3);
    assert_eq!(all_time.total_tokens, 4300);

    let daily = get_daily_breakdown(&conn, 7).await.unwrap();
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].requests, 3);
    assert_eq!(daily[0].total_tokens, 4300);

    let models = get_model_breakdown(&conn).await.unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].model, "deepseek-reasoner");
    assert_eq!(models[0].total_tokens, 3000);
    assert_eq!(models[1].model, "deepseek-chat");
    assert_eq!(models[1].total_tokens, 1300);
}
