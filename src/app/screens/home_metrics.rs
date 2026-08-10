use crate::Download;
use std::collections::VecDeque;
use std::sync::atomic::Ordering::Relaxed;

pub(super) struct Metrics {
    bandwidth_samples: VecDeque<f32>,
    request_samples: VecDeque<f32>,
    last_downloaded: usize,
    last_requests: usize,
}

impl Metrics {
    pub(super) fn new() -> Self {
        Self {
            bandwidth_samples: VecDeque::new(),
            request_samples: VecDeque::new(),
            last_downloaded: 0,
            last_requests: 0,
        }
    }

    pub(super) fn sample<'a>(&mut self, downloads: impl Iterator<Item = &'a Download>) {
        let (downloaded, requests) = downloads.fold((0, 0), |(downloaded, requests), download| {
            (
                downloaded + download.downloaded.load(Relaxed),
                requests + download.request_count(),
            )
        });

        self.push(
            downloaded.saturating_sub(self.last_downloaded) as f32 / 1_048_576.0,
            requests.saturating_sub(self.last_requests) as f32,
        );
        self.last_downloaded = downloaded;
        self.last_requests = requests;
    }

    pub(super) fn bandwidth_samples(&self) -> impl Iterator<Item = f32> + '_ {
        self.bandwidth_samples.iter().copied()
    }

    pub(super) fn request_samples(&self) -> impl Iterator<Item = f32> + '_ {
        self.request_samples.iter().copied()
    }

    fn push(&mut self, bandwidth: f32, requests: f32) {
        const SAMPLE_CAPACITY: usize = 60;

        self.bandwidth_samples.push_back(bandwidth);
        self.request_samples.push_back(requests);
        if self.bandwidth_samples.len() > SAMPLE_CAPACITY {
            self.bandwidth_samples.pop_front();
            self.request_samples.pop_front();
        }
    }
}
