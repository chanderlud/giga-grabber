# Giga Grabber
A fast and stable [Mega](https://mega.nz) downloader using Reqwest for HTTP and Iced for the UI
## Features
- Concurrent downloads with customizable concurrency budget to optimize performance across all file sizes
- Exponential backoff & timeouts, per-file cancellation & pausing, and other quality of life features
- In the event of a crash or other failure, partial downloads can be resumed with minimal data loss
- End-to-end encryption for all downloads
- Download speeds can exceed 70MB/s on a single worker in ideal conditions
- Cross-platform support (Windows, macOS, and Linux)
- Simple CLI for downloading files & folders

## CLI Usage
```shell
./giga-grabber <folder or file URL>
```

# Contributing
All contributions are welcome. I would especially appreciate help with the following:
- [ ] Upgrading Iced from 0.9 to the latest version is a high priority task
- [ ] Light / Dark theme improvements. Currently, there is no Light theme, even though it is supported.
- [ ] UI and UX improvements. I am not the best UI/UX designer, so any improvements are welcome.

# Warnings
- Using proxies or a VPN to bypass Mega's download limit is a violation of their [Terms of Service](https://mega.nz/terms)
- Giga Grabber can use a massive amount of bandwidth. Carefully consider the options you use
