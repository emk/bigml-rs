# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.6.4 - 2020-05-06

### Added

- `bigml-parallel`: Added new `--retry-on` and `--retry-count` arguments that can be used to retry failed executions.

### Fixed

- `bigml-parallel`: Removed `.timeout()` clauses that were probably unnecessary, because the code in question never returned `WaitStatus::Waiting`. This might slightly change retry behavior.
- Fixed lots of minor warnings from the newest `clippy` and Rust releases.