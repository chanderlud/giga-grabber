Changelog
=========

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

[Unreleased]
------------

### Added

### Changed

### Fixed

### Removed

[0.4.0] - 2023-04-12
--------------------

### Added

- Added `Client::{download_thumbnail, download_preview_image}`.
- Added `Client::{upload_thumbnail, upload_preview_image}`.
- Added `Node::{has_thumbnail, has_preview_image}`.
- Implemented `Default` trait for `ClientBuilder`.
- Added `NodeKind::{is_file, is_folder, is_root, is_rubbish_bin, is_inbox}`.

### Changed

- Changed `Error` to implement both `Send` and `Sync`.
- Slightly simplified `HttpClient` trait.

### Fixed

- Fixed issue with incorrect MAC computation.
- Added appropriate size limits on I/O readers and writers.

### Removed

- Removed `Client::move_to_rubbish_bin` function.

[0.3.0] - 2023-04-09
--------------------

### Added

- Added HTTPS usage (during downloads and uploads) as a configurable option.
- Added support for listing and downloading from public MEGA links.
- Added `Nodes` type to represent collections of fetched nodes.
- Exported `NodeKind` type.

### Changed

- Most `Client` functions can now be called concurrently.

### Fixed

- Fixed issues with MAC computation when uploading files.

[0.2.1] - 2023-04-02
--------------------

### Added

- Added `Client::move_to_rubbish_bin` function.

### Fixed

- Fixed an issue where errors could be encountered after some successful operations, like moving or renaming a node.

[0.2.0] - 2023-02-26
--------------------

### Added

- Initial library release.

[0.1.0] - 2017-06-30 [YANKED]
-----------------------------

Legacy yanked release.
