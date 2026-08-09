use super::{Home, Message};
use crate::mega_client::Node;
use crate::{Download, MegaFile};
use std::path::PathBuf;

#[test]
fn bulk_control_resumes_when_all_visible_downloads_are_paused() {
    let first_file = MegaFile::new(
        Node::test_file("paused-first", "first.bin", 1_024),
        PathBuf::from("downloads"),
    );
    let second_file = MegaFile::new(
        Node::test_file("paused-second", "second.bin", 2_048),
        PathBuf::from("downloads"),
    );
    let first_download = Download::new(&first_file);
    let second_download = Download::new(&second_file);
    first_download.pause();
    second_download.pause();
    let mut home = Home::new();
    home.add_active_download(first_download);
    home.add_active_download(second_download);

    assert!(matches!(
        home.bulk_control_message(),
        Message::ResumeDownloads
    ));
}

#[test]
fn bulk_control_pauses_when_all_visible_downloads_are_running() {
    let first_file = MegaFile::new(
        Node::test_file("running-first", "first.bin", 1_024),
        PathBuf::from("downloads"),
    );
    let second_file = MegaFile::new(
        Node::test_file("running-second", "second.bin", 2_048),
        PathBuf::from("downloads"),
    );
    let mut home = Home::new();
    home.add_active_download(Download::new(&first_file));
    home.add_active_download(Download::new(&second_file));

    assert!(matches!(
        home.bulk_control_message(),
        Message::PauseDownloads
    ));
}

#[test]
fn bulk_control_pauses_when_visible_downloads_are_mixed() {
    let paused_file = MegaFile::new(
        Node::test_file("paused", "paused.bin", 1_024),
        PathBuf::from("downloads"),
    );
    let running_file = MegaFile::new(
        Node::test_file("running", "running.bin", 2_048),
        PathBuf::from("downloads"),
    );
    let paused_download = Download::new(&paused_file);
    paused_download.pause();
    let mut home = Home::new();
    home.add_active_download(paused_download);
    home.add_active_download(Download::new(&running_file));

    assert!(matches!(
        home.bulk_control_message(),
        Message::PauseDownloads
    ));
}
