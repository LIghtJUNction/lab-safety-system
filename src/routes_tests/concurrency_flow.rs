use std::{sync::Arc, time::Instant};

use tokio::sync::Barrier;

use super::*;

fn print_latency_metrics(scenario: &str, durations_ms: &mut [u128], errors: usize) {
    durations_ms.sort_unstable();
    let count = durations_ms.len();
    let percentile = |percent: usize| {
        let index = (count.saturating_sub(1) * percent).div_ceil(100);
        durations_ms[index]
    };
    eprintln!(
        "CONCURRENCY_METRICS scenario={scenario} count={count} errors={errors} p50_ms={} p95_ms={} max_ms={}",
        percentile(50),
        percentile(95),
        durations_ms[count - 1]
    );
}

#[tokio::test]
async fn authenticated_reads_should_all_succeed_concurrently() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    const REQUEST_COUNT: usize = 20;
    let barrier = Arc::new(Barrier::new(REQUEST_COUNT));
    let mut handles = Vec::with_capacity(REQUEST_COUNT);
    for _ in 0..REQUEST_COUNT {
        let app = ctx.app.clone();
        let token = ctx.researcher_token.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let started = Instant::now();
            let (status, body) = request(
                &app,
                Method::GET,
                "/api/v1/auth/me",
                Some(&token),
                Body::empty(),
                None,
            )
            .await?;
            Ok::<_, anyhow::Error>((status, body, started.elapsed().as_millis()))
        }));
    }

    let mut statuses = Vec::with_capacity(REQUEST_COUNT);
    let mut durations = Vec::with_capacity(REQUEST_COUNT);
    for handle in handles {
        let (status, body, duration) = handle.await??;
        statuses.push((status, body));
        durations.push(duration);
    }
    let errors = statuses
        .iter()
        .filter(|(status, _)| *status != StatusCode::OK)
        .count();
    print_latency_metrics("authenticated_reads", &mut durations, errors);
    assert_eq!(errors, 0, "read responses: {statuses:?}");
    Ok(())
}

#[tokio::test]
async fn overlapping_bookings_should_have_exactly_one_success() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let lab_id = sqlx::query_scalar::<_, i64>(
        "insert into labs (code, name, status) values ($1, 'Concurrency lab', 'active') returning id",
    )
    .bind(format!("CONCURRENCY-{}", ctx.schema))
    .fetch_one(&ctx.pool)
    .await?;
    sqlx::query("insert into lab_users (lab_id, user_id, lab_role) values ($1, $2, 'lab_member')")
        .bind(lab_id)
        .bind(ctx.researcher_id)
        .execute(&ctx.pool)
        .await?;
    let equipment_id = sqlx::query_scalar::<_, i64>(
        r#"
        insert into equipment (asset_code, name, lab_id, lab_name, status)
        values ($1, 'Concurrent equipment', $2, 'Concurrency lab', 'available')
        returning id
        "#,
    )
    .bind(format!("CONCURRENT-{}", ctx.schema))
    .bind(lab_id)
    .fetch_one(&ctx.pool)
    .await?;

    const REQUEST_COUNT: usize = 8;
    let barrier = Arc::new(Barrier::new(REQUEST_COUNT));
    let mut handles = Vec::with_capacity(REQUEST_COUNT);
    for request_index in 0..REQUEST_COUNT {
        let app = ctx.app.clone();
        let token = ctx.researcher_token.clone();
        let user_id = ctx.researcher_id;
        let barrier = Arc::clone(&barrier);
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let started = Instant::now();
            let (status, body) = json_request(
                &app,
                Method::POST,
                "/api/v1/equipment-bookings",
                Some(&token),
                serde_json::json!({
                    "equipment_id": equipment_id,
                    "user_id": user_id,
                    "starts_at": "2026-07-14T02:00:00Z",
                    "ends_at": "2026-07-14T04:00:00Z",
                    "purpose": format!("Concurrent request {request_index}")
                }),
            )
            .await?;
            Ok::<_, anyhow::Error>((status, body, started.elapsed().as_millis()))
        }));
    }

    let mut responses = Vec::with_capacity(REQUEST_COUNT);
    let mut durations = Vec::with_capacity(REQUEST_COUNT);
    for handle in handles {
        let (status, body, duration) = handle.await??;
        responses.push((status, body));
        durations.push(duration);
    }
    let success_count = responses
        .iter()
        .filter(|(status, _)| *status == StatusCode::OK)
        .count();
    let conflict_count = responses
        .iter()
        .filter(|(status, _)| *status == StatusCode::CONFLICT)
        .count();
    let errors = REQUEST_COUNT - success_count - conflict_count;
    print_latency_metrics("overlapping_bookings", &mut durations, errors);
    eprintln!(
        "BOOKING_RESULTS success={success_count} conflict={conflict_count} unexpected={errors}"
    );
    assert_eq!(success_count, 1, "booking responses: {responses:?}");
    assert_eq!(
        conflict_count,
        REQUEST_COUNT - 1,
        "booking responses: {responses:?}"
    );
    Ok(())
}
