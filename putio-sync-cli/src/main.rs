//! # putio-sync CLI
//!
//! Command-line interface for putio-sync

mod cli;

use cli::{Cli, Commands};

fn main() {
    let args = Cli::parse_args();

    // Display configuration for debugging
    println!("putio-sync v{}", env!("CARGO_PKG_VERSION"));

    if let Some(config_path) = &args.config {
        println!("Config path: {}", config_path.display());
    }

    println!("Verbosity level: {}", args.verbose);
    println!("Quiet mode: {}", args.quiet);

    // Handle commands
    match args.command {
        Commands::Login { token, interactive, no_verify } => {
            println!("\nLogin command:");
            if let Some(t) = token {
                println!("  Token provided: {}...", &t[..t.len().min(8)]);
            }
            if interactive {
                println!("  Interactive mode: enabled");
            }
            println!("  Verify token: {}", !no_verify);
            println!("\nLogin functionality coming soon!");
        }
        Commands::List {
            path,
            id,
            recursive,
            long,
            filter,
            file_type,
            min_size,
            max_size,
            sort,
            reverse,
            output,
        } => {
            println!("\nList command:");
            if let Some(p) = path {
                println!("  Path: {}", p);
            } else if let Some(i) = id {
                println!("  ID: {}", i);
            } else {
                println!("  Path: / (root)");
            }

            if recursive {
                println!("  Recursive: enabled");
            }
            if long {
                println!("  Long format: enabled");
            }
            if let Some(f) = filter {
                println!("  Filter: {}", f);
            }
            if let Some(t) = file_type {
                println!("  Type: {}", t);
            }
            if let Some(s) = min_size {
                println!("  Min size: {}", s);
            }
            if let Some(s) = max_size {
                println!("  Max size: {}", s);
            }
            if let Some(s) = sort {
                println!("  Sort by: {}", s);
            }
            if reverse {
                println!("  Reverse: enabled");
            }
            println!("  Output format: {}", output);

            println!("\nList functionality coming soon!");
        }
    }
}
