//!
//! Example program that simply moves a file from MEGA to a different folder.
//!

use std::env;

async fn run(mega: &mut mega::Client, distant_file_path: &str, new_name: &str) -> mega::Result<()> {
    let nodes = mega.fetch_own_nodes().await?;

    let node = nodes
        .get_node_by_path(distant_file_path)
        .expect("could not find node by path");

    mega.rename_node(node, new_name).await?;

    println!("node successfully renamed !");

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let email = env::var("MEGA_EMAIL").expect("missing MEGA_EMAIL environment variable");
    let password = env::var("MEGA_PASSWORD").expect("missing MEGA_PASSWORD environment variable");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [distant_file_path, new_name] = args.as_slice() else {
        panic!("expected 2 command-line arguments: {{distant_file_path}} {{new_name}}");
    };

    let http_client = reqwest::Client::new();
    let mut mega = mega::Client::builder().build(http_client).unwrap();

    mega.login(&email, &password, None).await.unwrap();

    let result = run(&mut mega, distant_file_path, new_name).await;
    mega.logout().await.unwrap();

    result.unwrap();
}
