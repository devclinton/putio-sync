# Changelog

All notable changes to putio-sync-client will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2025-12-07

### Added
- Initial release of standalone putio-sync-client crate
- Pure API client extracted from putio-sync-core
- Support for OAuth token authentication (Bearer)
- Support for app-specific password authentication (Basic Auth)
- Comprehensive API coverage:
  - Account operations (get_account_info, verify_token)
  - File operations (list_files, get_file_details, get_download_url, upload_file)
  - Transfer operations (add_transfer_url, add_transfer_file, list_transfers, get_transfer_info, cancel_transfers, retry_transfer)
  - Delete operations (delete_files, delete_folder_recursive)
  - Path operations (resolve_path, is_folder)
- Automatic retry with exponential backoff for failed requests
- Extended timeout support for file downloads
- Comprehensive error handling with helpful error messages
- Full test coverage with mockito-based tests

### Changed
- Renamed `get_file` to `get_file_details` for clarity
- Renamed `get_transfer` to `get_transfer_info` for clarity
- Made `get_auth_header` public for advanced use cases
- Improved error messages with troubleshooting suggestions

### Migrated
- Extracted from putio-sync-core 0.1.1 as part of crate restructuring
- All API client functionality now in standalone crate
