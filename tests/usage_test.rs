use deeperseeker::domain::usage::format_metric;
use deeperseeker::infra::db::{init_db, open_db};
use deeperseeker::infra::usage_db::{
    get_all_summaries, get_daily_breakdown, get_model_breakdown, record_usage,
};

#[test]
fn test_metric_formatting() {
    assert_eq!(format_metric(0, false), "0");
    assert_eq!(format_metric(84, false), "84");
    assert_eq!(format_metric(500, false), "500");
    assert_eq!(format_metric(1_500, false), "1.5K");
    assert_eq!(format_metric(44_850, false), "44.9K");
    assert_eq!(format_metric(44_934, false), "44.9K");
    assert_eq!(format_metric(85_524, false), "85.5K");
    assert_eq!(format_metric(2_140_000, false), "2.14M");
    assert_eq!(format_metric(1_050_000_000, false), "1.05B");
    assert_eq!(format_metric(2_140_500, true), "2140500");
}

#[tokio::test]
async fn test_db_usage_recording_and_summaries() {
    let conn = open_db(":memory:").await.unwrap();
    init_db(&conn).await.unwrap();

    // Record usage
    record_usage(&conn, "deepseek-chat", 100, 200, 50, Some(1))
        .await
        .unwrap();
    record_usage(&conn, "deepseek-chat", 400, 600, 300, Some(1))
        .await
        .unwrap();
    record_usage(&conn, "deepseek-reasoner", 1000, 2000, 800, Some(2))
        .await
        .unwrap();

    let summaries = get_all_summaries(&conn).await.unwrap();
    assert_eq!(summaries.len(), 6);

    let today = summaries.iter().find(|s| s.period == "Today").unwrap();
    assert_eq!(today.requests, 3);
    assert_eq!(today.prompt_tokens, 1500);
    assert_eq!(today.cached_tokens, 1150);
    assert_eq!(today.completion_tokens, 2800);
    assert_eq!(today.total_tokens, 4300);
    assert!((today.cache_hit_rate() - 76.66).abs() < 0.1);

    let all_time = summaries.iter().find(|s| s.period == "All Time").unwrap();
    assert_eq!(all_time.requests, 3);
    assert_eq!(all_time.cached_tokens, 1150);
    assert_eq!(all_time.total_tokens, 4300);

    let daily = get_daily_breakdown(&conn, 7).await.unwrap();
    assert_eq!(daily.len(), 1);
    assert_eq!(daily[0].requests, 3);
    assert_eq!(daily[0].cached_tokens, 1150);
    assert_eq!(daily[0].total_tokens, 4300);

    let models = get_model_breakdown(&conn).await.unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].model, "deepseek-reasoner");
    assert_eq!(models[0].total_tokens, 3000);
    assert_eq!(models[1].model, "deepseek-chat");
    assert_eq!(models[1].total_tokens, 1300);

    let token_usages = deeperseeker::infra::usage_db::get_token_usages(&conn).await.unwrap();
    assert_eq!(token_usages.len(), 2);
    let t1 = token_usages.iter().find(|u| u.token_id == 1).unwrap();
    assert_eq!(t1.requests, 2);
    assert_eq!(t1.prompt_tokens, 500);
    assert_eq!(t1.completion_tokens, 800);
    assert_eq!(t1.total_tokens, 1300);
    assert_eq!(t1.cached_tokens, 350);
    assert!((t1.cache_hit_rate() - 70.0).abs() < 0.1);

    let t2 = token_usages.iter().find(|u| u.token_id == 2).unwrap();
    assert_eq!(t2.requests, 1);
    assert_eq!(t2.total_tokens, 3000);
}

#[tokio::test]
async fn test_filtered_usage_queries() {
    let conn = open_db(":memory:").await.unwrap();
    init_db(&conn).await.unwrap();

    record_usage(&conn, "deepseek-chat", 100, 200, 50, Some(1))
        .await
        .unwrap();
    record_usage(&conn, "deepseek-chat", 400, 600, 300, Some(1))
        .await
        .unwrap();
    record_usage(&conn, "deepseek-reasoner", 1000, 2000, 800, Some(2))
        .await
        .unwrap();

    let chat_filter = deeperseeker::domain::usage::UsageFilter {
        model: Some("deepseek-chat".to_string()),
        token_id: None,
    };
    let chat_summaries = deeperseeker::infra::usage_db::get_filtered_summaries(&conn, &chat_filter)
        .await
        .unwrap();
    let chat_today = chat_summaries.iter().find(|s| s.period == "Today").unwrap();
    assert_eq!(chat_today.requests, 2);
    assert_eq!(chat_today.cached_tokens, 350);
    assert_eq!(chat_today.total_tokens, 1300);

    let token2_filter = deeperseeker::domain::usage::UsageFilter {
        model: None,
        token_id: Some(2),
    };
    let token2_summaries =
        deeperseeker::infra::usage_db::get_filtered_summaries(&conn, &token2_filter)
            .await
            .unwrap();
    let token2_today = token2_summaries
        .iter()
        .find(|s| s.period == "Today")
        .unwrap();
    assert_eq!(token2_today.requests, 1);
    assert_eq!(token2_today.cached_tokens, 800);
    assert_eq!(token2_today.total_tokens, 3000);
}

#[test]
fn test_dashboard_template_metrics_rendering() {
    let mut tera = tera::Tera::default();
    let template_content = std::fs::read_to_string("templates/dashboard.html").unwrap();
    let base_content = std::fs::read_to_string("templates/base.html").unwrap();
    tera.add_raw_template("base.html", &base_content).unwrap();
    tera.add_raw_template("dashboard.html", &template_content)
        .unwrap();

    let mut ctx = tera::Context::new();
    let summaries = vec![deeperseeker::api::dashboard::DashboardSummaryView {
        period: "Today".to_string(),
        requests: "15".to_string(),
        prompt_tokens: "44.9K".to_string(),
        cached_tokens: "40.0K".to_string(),
        cache_rate: "89.1%".to_string(),
        completion_tokens: "84".to_string(),
        total_tokens: "44.9K".to_string(),
        raw_requests: 15,
        raw_prompt_tokens: 44_850,
        raw_cached_tokens: 40_000,
        raw_completion_tokens: 84,
        raw_total_tokens: 44_934,
    }];
    let tokens = vec![deeperseeker::api::dashboard::DashboardTokenView {
        id: 1,
        alias: Some("account_alpha".to_string()),
        masked: "user...9999".to_string(),
        status: "ACTIVE".to_string(),
        requests: "12".to_string(),
        prompt_tokens: "34.5K".to_string(),
        completion_tokens: "1.2K".to_string(),
        total_tokens: "35.7K".to_string(),
        cached_tokens: "20.0K".to_string(),
        cache_rate: "58.0%".to_string(),
    }];
    ctx.insert("summaries", &summaries);
    ctx.insert("tokens", &tokens);
    ctx.insert("active_count", &1);
    ctx.insert("total_tokens_count", &1);
    ctx.insert("in_flight", &0);
    ctx.insert("cache_hit_rate", &"89.1%");
    ctx.insert("today_cached", &"40.0K");
    ctx.insert("port", &4000);
    ctx.insert("api_key", &"dseeker");

    let rendered = tera.render("dashboard.html", &ctx).unwrap();
    assert!(rendered.contains("44.9K"));
    assert!(rendered.contains("40.0K"));
    assert!(rendered.contains("89.1%"));
    assert!(rendered.contains("title=\"44934 tokens\""));
    assert!(rendered.contains("title=\"44850 tokens\""));
    assert!(rendered.contains("title=\"84 tokens\""));
    assert!(rendered.contains("title=\"15 requests\""));
    assert!(rendered.contains("#1"));
    assert!(rendered.contains("account_alpha"));
    assert!(rendered.contains("35.7K"));
    assert!(rendered.contains("58.0%"));
    assert!(rendered.contains("user...9999"));
}
