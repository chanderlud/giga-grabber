//!
//! Example program that displays all of the available nodes in MEGA
//! in a textual tree format.
//!

use std::env;

use text_trees::{FormatCharacters, StringTreeNode, TreeFormatting};

fn construct_tree_node(nodes: &mega::Nodes, node: &mega::Node) -> StringTreeNode {
    let (mut folders, mut files): (Vec<_>, Vec<_>) = node
        .children()
        .iter()
        .filter_map(|hash| nodes.get_node_by_hash(hash))
        .partition(|node| node.kind().is_folder());

    folders.sort_unstable_by_key(|node| node.name());
    files.sort_unstable_by_key(|node| node.name());

    let children = std::iter::empty()
        .chain(folders)
        .chain(files)
        .map(|node| construct_tree_node(nodes, node));

    StringTreeNode::with_child_nodes(node.name().to_string(), children)
}

async fn run(mega: &mut mega::Client, distant_file_path: Option<&str>) -> mega::Result<()> {
    let mut stdout = std::io::stdout().lock();

    let nodes = mega.fetch_own_nodes().await?;

    if let Some(distant_file_path) = distant_file_path {
        let root = nodes
            .get_node_by_path(distant_file_path)
            .expect("could not get root node");

        let tree = construct_tree_node(&nodes, root);
        let formatting = TreeFormatting::dir_tree(FormatCharacters::box_chars());

        println!();
        tree.write_with_format(&mut stdout, &formatting).unwrap();
        println!();
    } else {
        let cloud_drive = nodes.cloud_drive().expect("could not get Cloud Drive root");
        let inbox = nodes.inbox().expect("could not get Inbox root");
        let rubbish_bin = nodes.rubbish_bin().expect("could not get Rubbish Bin root");

        let cloud_drive_tree = construct_tree_node(&nodes, cloud_drive);
        let inbox_tree = construct_tree_node(&nodes, inbox);
        let rubbish_bin_tree = construct_tree_node(&nodes, rubbish_bin);

        let formatting = TreeFormatting::dir_tree(FormatCharacters::box_chars());

        println!();
        cloud_drive_tree
            .write_with_format(&mut stdout, &formatting)
            .unwrap();
        println!();
        inbox_tree
            .write_with_format(&mut stdout, &formatting)
            .unwrap();
        println!();
        rubbish_bin_tree
            .write_with_format(&mut stdout, &formatting)
            .unwrap();
        println!();
    }

    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let email = env::var("MEGA_EMAIL").expect("missing MEGA_EMAIL environment variable");
    let password = env::var("MEGA_PASSWORD").expect("missing MEGA_PASSWORD environment variable");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let distant_file_path = match args.as_slice() {
        [] => None,
        [distant_file_path] => Some(distant_file_path.as_str()),
        _ => {
            panic!("expected 0 or 1 command-line arguments: {{distant_file_path}}");
        }
    };

    let http_client = reqwest::Client::new();
    let mut mega = mega::Client::builder().build(http_client).unwrap();

    mega.login(&email, &password, None).await.unwrap();

    let result = run(&mut mega, distant_file_path).await;
    mega.logout().await.unwrap();

    result.unwrap();
}
