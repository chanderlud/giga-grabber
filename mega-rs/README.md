MEGA API Rust Client
====================

<!-- [![CI](https://github.com/Hirevo/mega-rs/actions/workflows/ci.yaml/badge.svg)](https://github.com/Hirevo/mega-rs/actions/workflows/ci.yaml) -->
[![version](https://img.shields.io/crates/v/mega)](https://crates.io/crates/mega)
[![docs](https://img.shields.io/docsrs/mega)](https://docs.rs/mega)
[![license](https://img.shields.io/crates/l/mega)](https://github.com/Hirevo/mega-rs#license)

This is an API client library for interacting with MEGA's API using Rust.

Features
--------

- [x] Login with MEGA
  - [x] MFA support
- [x] Get storage quotas
- [x] Listing nodes
- [x] Downloading nodes
- [x] Uploading nodes
- [x] Creating folders
- [x] Renaming, moving and deleting nodes
- [x] Timeout support
- [x] Retries (exponential-backoff) support
- [ ] Parallel connections (downloading/uploading multiple file chunks in parallel)
- [x] Downloading thumbnails and preview images
- [x] Uploading thumbnails and preview images
- [x] Shared links support
  - [x] Downloading from shared links
  - [ ] Uploading to shared folders
  - [ ] Create shared links to owned nodes
- [ ] Server-to-Client events support

Examples
--------

You can see examples of how to use this library by looking at [**the different examples available**](https://github.com/Hirevo/mega-rs/tree/main/examples).

License
-------

Licensed under either of

- Apache License, Version 2.0 (LICENSE-APACHE or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license (LICENSE-MIT or <http://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
