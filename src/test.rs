use std::{
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use httpmock::{Method::GET, MockServer};
use reqwest::Url;
use reqwest_middleware::ClientBuilder;
use time::{format_description::well_known::Rfc2822, OffsetDateTime};

use crate::RetryAfterMiddleware;

#[tokio::test]
async fn test() {
    // create
    let test_duration = Duration::from_secs(2);
    let middleware = Arc::new(RetryAfterMiddleware::new());

    // build client with middleware
    let client = ClientBuilder::new(reqwest::Client::new())
        .with_arc(middleware.clone())
        .build();

    test_empty_retry_after(&middleware).await;

    // create mock server
    let server = MockServer::start();
    let pre_ra_mock = server.mock(|when, then| {
        when.method(GET).path("/").header("RA", "true");
        then.status(200)
            .header("Retry-After", test_duration.as_secs().to_string())
            .body("");
    });
    let post_ra_mock = server.mock(|when, then| {
        when.method(GET).path("/");
        then.status(200).body("");
    });
    let normal_mock = server.mock(|when, then| {
        when.method(GET).path("/normal");
        then.status(200).body("");
    });

    let url = Url::from_str(&server.url("/")).unwrap();

    // hit URL; get RA value and store it
    let pre_test = SystemTime::now();
    client
        .get(url.clone())
        .header("RA", "true")
        .send()
        .await
        .unwrap();
    pre_ra_mock.assert_async().await;
    test_valid_retry_after(&middleware, &url, pre_test, test_duration).await;

    // hit other URL, which should return instantly
    let normal = Url::from_str(&server.url("/normal")).unwrap();
    let before_normal = SystemTime::now();
    client.get(normal.clone()).send().await.unwrap();
    normal_mock.assert_async().await;
    assert!(
        SystemTime::now()
            .duration_since(before_normal)
            .unwrap()
            .as_secs_f64()
            <= 0.2
    );
    test_absent_retry_after(&middleware, &normal).await;

    // hit URL with stored RA
    client.get(url.clone()).send().await.unwrap();
    post_ra_mock.assert_async().await;

    // this should have (1) slept and (2) cleared the stored RA afterward
    let post_test = SystemTime::now();
    assert!(post_test.duration_since(pre_test).unwrap() >= test_duration);
    test_empty_retry_after(&middleware).await;
}

#[tokio::test]
async fn test_rfc2822() {
    let mut test_duration = Duration::from_secs(2);

    // Build server and client
    let server = MockServer::start();
    let middleware = Arc::new(RetryAfterMiddleware::new());
    let client = ClientBuilder::new(reqwest::Client::new())
        .with_arc(middleware.clone())
        .build();

    // Conversion to RFC 2822 floors the duration, so compensate with ceiling function.
    let begin =
        OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() + Duration::from_secs(1);
    let ra = begin + test_duration;
    test_duration = (begin - ra).unsigned_abs();

    let ra_mock = server.mock(|when, then| {
        when.method(GET).path("/").header("RA", "true");
        then.status(200)
            .header("Retry-After", ra.format(&Rfc2822).unwrap())
            .body("");
    });
    let no_ra_mock = server.mock(|when, then| {
        when.method(GET).path("/");
        then.status(200).body("");
    });

    // hit URL; store RA value
    let url = Url::from_str(&server.url("/")).unwrap();
    client
        .get(url.clone())
        .header("RA", "true")
        .send()
        .await
        .unwrap();
    test_valid_retry_after(&middleware, &url, SystemTime::now(), test_duration).await;
    ra_mock.assert_async().await;

    // hit URL with stored RA
    client.get(url.clone()).send().await.unwrap();
    no_ra_mock.assert_async().await;

    // this should have (1) slept and (2) cleared the stored RA afterward
    let duration = SystemTime::now().duration_since(begin.into()).unwrap();
    assert!(duration >= test_duration);
    test_empty_retry_after(&middleware).await;
}

async fn test_valid_retry_after(
    middleware: &Arc<RetryAfterMiddleware>,
    url: &Url,
    now: SystemTime,
    test_duration: Duration,
) {
    let time = middleware
        .retry_after
        .read()
        .await
        .get(url)
        .cloned()
        .unwrap();
    let duration = time.duration_since(now).unwrap();
    assert!(duration >= test_duration);
}

async fn test_absent_retry_after(middleware: &Arc<RetryAfterMiddleware>, url: &Url) {
    assert!(middleware.retry_after.read().await.get(url).is_none());
}

async fn test_empty_retry_after(middleware: &Arc<RetryAfterMiddleware>) {
    assert!(middleware.retry_after.read().await.is_empty());
}

