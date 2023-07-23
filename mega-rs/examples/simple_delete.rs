//!
//! Example program that simply deletes a file from MEGA.
//!

use std::env;

async fn run(mega: &mut mega::Client, distant_file_path: &str) -> mega::Result<()> {
    let nodes = mega.fetch_own_nodes().await?;

    let node = nodes
        .get_node_by_path(distant_file_path)
        .expect("could not find node by path");

    mega.delete_node(node).await?;

    println!("node successfully deleted !");

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let email = env::var("MEGA_EMAIL").expect("missing MEGA_EMAIL environment variable");
    let password = env::var("MEGA_PASSWORD").expect("missing MEGA_PASSWORD environment variable");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [distant_file_path] = args.as_slice() else {
        panic!("expected 1 command-line argument: {{distant_file_path}}");
    };

    let http_client = reqwest::Client::new();
    let mut mega = mega::Client::builder().build(http_client).unwrap();

    mega.login(&email, &password, None).await.unwrap();

    let result = run(&mut mega, distant_file_path).await;
    mega.logout().await.unwrap();

    result.unwrap();
}
