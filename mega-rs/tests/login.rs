//!
//! Integration test for simply logging in and out of MEGA.
//!

use std::env;

#[tokio::test]
async fn login_and_logout_test() {
    let email = env::var("MEGA_EMAIL").expect("missing MEGA_EMAIL environment variable");
    let password = env::var("MEGA_PASSWORD").expect("missing MEGA_PASSWORD environment variable");

    let http_client = reqwest::Client::new();
    let mut mega = mega::Client::builder().build(http_client).unwrap();

    mega.login(&email, &password, None)
        .await
        .expect("could not log in to MEGA");

    mega.logout().await.expect("could not log out from MEGA");
}
