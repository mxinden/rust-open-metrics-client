# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.11.2] - 2021-06-09
### Fixed
- Do not separate labels with spaces.

## [0.11.1] - 2021-06-08
### Fixed
- Encode Info metric labels.

## [0.11.0] - 2021-06-08
### Added
- Add support for OpenMetrics Info metrics (see [PR 18]).

[PR 18]: https://github.com/mxinden/rust-open-metrics-client/pull/18

## [0.10.1] - 2021-05-31
### Added
- Implement `Encode` for `u32`.

### Fixed
- Update to open-metrics-client-derive-text-encode v0.1.1 which handles keyword
  identifiers aka raw identifiers

  https://github.com/mxinden/rust-open-metrics-client/pull/16

## [0.10.0] - 2021-04-29
### Added
- Added `metrics::histogram::linear_buckets`.
  https://github.com/mxinden/rust-open-metrics-client/issues/13

### Changed
- Renamed `metrics::histogram::exponential_series` to
  `metrics::histogram::exponential_buckets`.
  https://github.com/mxinden/rust-open-metrics-client/issues/13
