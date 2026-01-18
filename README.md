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
Usage: giga_grabber.exe [OPTIONS] <URL>

Arguments:
  <URL>
          MEGA URL to download

Options:
      --max-workers <MAX_WORKERS>
          Maximum number of concurrent download workers (1-10, default: 10)

      --concurrency-budget <CONCURRENCY_BUDGET>
          Concurrency budget for weighted downloads (1-100, default: 10)

      --max-retries <MAX_RETRIES>
          Maximum number of retry attempts (default: 3)

      --timeout <TIMEOUT>
          Request timeout in seconds (default: 20)

      --max-retry-delay <MAX_RETRY_DELAY>
          Maximum retry delay in seconds (default: 30)

      --min-retry-delay <MIN_RETRY_DELAY>
          Minimum retry delay in seconds (default: 10)

      --proxy-mode <PROXY_MODE>
          Proxy mode: none, single, or random (default: none)

          Possible values:
          - none:   No proxy
          - single: Use a single proxy
          - random: Use a random proxy from the list

      --proxies <PROXIES>
          Proxy URL (can be specified multiple times for random mode)

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

## Contributing
All contributions are welcome

## Warnings
- Using proxies or a VPN to bypass Mega's download limit is a violation of their [Terms of Service](https://mega.nz/terms)
- Giga Grabber can use a massive amount of bandwidth. Carefully consider the options you use
