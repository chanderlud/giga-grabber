//!
//! Integration test for uploading a file to MEGA and downloading it back.
//!

use std::env;

use rand::distributions::{Alphanumeric, DistString};

#[tokio::test]
async fn upload_and_download_test() {
    let email = env::var("MEGA_EMAIL").expect("missing MEGA_EMAIL environment variable");
    let password = env::var("MEGA_PASSWORD").expect("missing MEGA_PASSWORD environment variable");

    let http_client = reqwest::Client::new();
    let mut mega = mega::Client::builder().build(http_client).unwrap();

    mega.login(&email, &password, None)
        .await
        .expect("could not log in to MEGA");

    let nodes = mega
        .fetch_own_nodes()
        .await
        .expect("could not fetch own nodes");

    let root = nodes
        .cloud_drive()
        .expect("could not find Cloud Drive root");

    let uploaded = {
        let mut rng = rand::thread_rng();
        Alphanumeric.sample_string(&mut rng, 1024)
    };

    let size = uploaded.len();

    mega.upload_node(
        root,
        "mega-rs-test-file.txt",
        size as _,
        uploaded.as_bytes(),
    )
    .await
    .expect("could not upload test file");

    let nodes = mega
        .fetch_own_nodes()
        .await
        .expect("could not fetch own nodes (after upload)");

    let node = nodes
        .get_node_by_path("/Root/mega-rs-test-file.txt")
        .expect("could not find test file node after upload");

    let mut downloaded = Vec::default();
    mega.download_node(node, &mut downloaded)
        .await
        .expect("could not download test file");

    assert_eq!(uploaded.as_bytes(), downloaded.as_slice());

    mega.delete_node(node)
        .await
        .expect("could not delete test file");

    mega.logout().await.expect("could not log out from MEGA");
}
