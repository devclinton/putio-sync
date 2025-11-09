Language:
    * Rust

Development Style:
    * Commit after completing each feature.
    * Test driven development
    * Document as we go
    * Use structured logging (e.g. via a logging/tracing crate) from day one.
    * Robust Error Handling

Architecture:
    * putio-sync-core
        * All domain logic:
        * Put.io API client (thin wrapper around reqwest).
        * Sync planner (what to download/update/delete).
        * Local state store (SQLite).
        * Config + auth handling.
        * Stats(Total downloads, total size, average transfer speed) and history
        * Download logic should support chunked downloads(eg when enabled and chunk size set to 256mb, when downloading a 1025mb file, it would be chunked into 5 parts and joined in the end)
        * Supports resume downloads
        * Blacklisting files
        * Support add torrents from torrent file or magnet links
        * No UI, no CLI parsing, no HTTP server.
    * putio-sync-cli
	    * Depends on putio-sync-core.
	    * Provides commands: login, list, status, sync, watch, etc.
	    * Pure binary.
    * putio-sync-web (server mode)
	    * Depends on putio-sync-core.
	    * Runs an HTTP server (Axum/Warp/Actix) + serves a simple API and react UI
	    * Exposes endpoints for:
	        * Starting sync
            * Change sync order
	        * Viewing progress/logs
            * View Stats
	        * Managing config (token, paths, filters)
	    * Can be built:
	        * as its own bin, or
	        * merged with CLI into one putio-sync binary with subcommands (putio-sync cli ..., putio-sync web ...).

Distribution:
    * Compiled binary for mac, linux and windows
    * Docker image