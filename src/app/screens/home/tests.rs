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

#[test]
fn metric_sampling_tracks_downloaded_bytes_and_requests_per_second() {
    let file = MegaFile::new(
        Node::test_file("metrics", "archive.iso", 4 * 1_024 * 1_024),
        PathBuf::from("downloads"),
    );
    let download = Download::new(&file);
    download.set_downloaded(1_048_576);
    download.record_request();
    download.record_request();
    let mut home = Home::new();
    home.add_active_download(download);

    home.sample_metrics();
    home.sample_metrics();

    assert_eq!(
        home.metrics.bandwidth_samples().collect::<Vec<_>>(),
        [1.0, 0.0]
    );
    assert_eq!(
        home.metrics.request_samples().collect::<Vec<_>>(),
        [2.0, 0.0]
    );
}

#[test]
fn completed_downloads_are_sampled_after_they_leave_the_active_list() {
    let file = MegaFile::new(
        Node::test_file("completed", "completed.iso", 1_024 * 1_024),
        PathBuf::from("downloads"),
    );
    let download = Download::new(&file);
    download.set_downloaded(524_288);
    let handle = download.node.handle.clone();
    let mut home = Home::new();
    home.add_active_download(download);

    home.remove_active_download(&handle);
    home.sample_metrics();

    assert_eq!(home.metrics.bandwidth_samples().collect::<Vec<_>>(), [0.5]);
    assert_eq!(home.metrics.request_samples().collect::<Vec<_>>(), [0.0]);
}

#[test]
fn metric_history_spans_a_gap_between_queued_downloads() {
    let first = MegaFile::new(
        Node::test_file("first", "first.iso", 2 * 1_024 * 1_024),
        PathBuf::from("downloads"),
    );
    let second = MegaFile::new(
        Node::test_file("second", "second.iso", 3 * 1_024 * 1_024),
        PathBuf::from("downloads"),
    );
    let first_download = Download::new(&first);
    first_download.set_downloaded(1_048_576);
    let first_handle = first_download.node.handle.clone();
    let second_download = Download::new(&second);
    second_download.set_downloaded(2 * 1_048_576);
    let mut home = Home::new();
    home.add_active_download(first_download);
    home.sample_metrics();

    home.remove_active_download(&first_handle);
    home.add_active_download(second_download);
    home.sample_metrics();

    assert_eq!(
        home.metrics.bandwidth_samples().collect::<Vec<_>>(),
        [1.0, 2.0]
    );
}
