//! The HTTP client against the fixture server: Digest negotiation end to end.
#![cfg(all(feature = "http", feature = "fixture"))]

use cctv_transport::fixture::{FixtureServer, Route};
use cctv_transport::http::{HttpClient, HttpConfig, Method};

#[tokio::test]
async fn digest_challenge_is_answered_and_the_request_retried() {
    let server =
        FixtureServer::start_with_digest(vec![Route::text("GET", "/hello", "hi")], "admin")
            .await
            .unwrap();
    let client = HttpClient::new(HttpConfig {
        username: Some("admin".into()),
        password: Some("pw".into()),
        ..HttpConfig::default()
    })
    .unwrap();
    let resp = client
        .request(
            Method::Get,
            &format!("{}/hello?x=1", server.url()),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.text(), "hi");
    let reqs = server.requests();
    assert_eq!(reqs.len(), 2, "one challenge, one answer");
    let auth = reqs[1].header("authorization").unwrap();
    assert!(auth.contains("uri=\"/hello?x=1\""), "{auth}");
    assert!(auth.contains("opaque=\"fixture-opaque\""), "{auth}");
}

#[tokio::test]
async fn without_credentials_the_401_is_returned_not_retried() {
    let server = FixtureServer::start_with_digest(vec![Route::text("GET", "/x", "x")], "admin")
        .await
        .unwrap();
    let client = HttpClient::new(HttpConfig::default()).unwrap();
    let resp = client
        .request(Method::Get, &format!("{}/x", server.url()), None, None)
        .await
        .unwrap();
    assert_eq!(resp.status, 401);
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn routes_select_by_method_path_and_body() {
    let server = FixtureServer::start(vec![
        Route::soap("/onvif", "GetProfiles", "<profiles/>"),
        Route::soap("/onvif", "GetStreamUri", "<uri/>"),
        Route::xml("PUT", "/ptz/*", "<ok/>"),
    ])
    .await
    .unwrap();
    let client = HttpClient::new(HttpConfig::default()).unwrap();
    let url = format!("{}/onvif", server.url());
    let a = client
        .request(Method::Post, &url, None, Some(b"<GetStreamUri/>".to_vec()))
        .await
        .unwrap();
    assert_eq!(a.text(), "<uri/>");
    let b = client
        .request(Method::Put, &format!("{}/ptz/1", server.url()), None, None)
        .await
        .unwrap();
    assert_eq!(b.text(), "<ok/>");
    let c = client
        .request(Method::Get, &format!("{}/nope", server.url()), None, None)
        .await
        .unwrap();
    assert_eq!(c.status, 404);
}

#[tokio::test]
async fn an_unreachable_host_is_a_connect_error() {
    let client = HttpClient::new(HttpConfig {
        timeout: std::time::Duration::from_secs(2),
        ..HttpConfig::default()
    })
    .unwrap();
    let err = client
        .request(Method::Get, "http://127.0.0.1:1/", None, None)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        cctv_transport::http::HttpError::Connect(_) | cctv_transport::http::HttpError::Timeout
    ));
}
