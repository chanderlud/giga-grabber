//!
//! Integration test for fetching nodes from public MEGA URLs.
//!

use std::env;

#[tokio::test]
async fn public_url_fetch_test() {
    let public_url =
        env::var("MEGA_PUBLIC_URL").expect("missing MEGA_PUBLIC_URL environment variable");

    let http_client = reqwest::Client::new();
    let mega = mega::Client::builder().build(http_client).unwrap();

    let _nodes = mega
        .fetch_public_nodes(&public_url)
        .await
        .expect("could not fetch nodes from public URL");
}
