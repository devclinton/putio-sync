//! Path utilities for handling various path formats
//!
//! Supports:
//! - Absolute paths: `/downloads`
//! - Relative paths: `./downloads`, `downloads`  
//! - Home directory: `~/Downloads`
//! - Current directory: `.`

use std::env;
use std::path::{Path, PathBuf};
use crate::error::{CoreError, CoreResult};

/// Expand a path that may contain tilde (~) or be relative
/// 
/// # Examples
/// 
/// ```
/// use putio_sync_core::path_utils::expand_path;
/// use std::path::PathBuf;
/// 
/// // Absolute path - unchanged
/// assert_eq!(expand_path("/downloads").unwrap(), PathBuf::from("/downloads"));
/// 
/// // Relative path - resolved from current directory
/// let current = std::env::current_dir().unwrap();
/// assert_eq!(expand_path("./downloads").unwrap(), current.join("downloads"));
/// 
/// // Home directory expansion
/// if let Ok(home) = std::env::var("HOME") {
///     assert_eq!(expand_path("~/Downloads").unwrap(), PathBuf::from(home).join("Downloads"));
/// }
/// ```
pub fn expand_path<P: AsRef<Path>>(path: P) -> CoreResult<PathBuf> {
    let path = path.as_ref();
    let path_str = path.to_string_lossy();
    
    let expanded = if path_str == "~" {
        // Handle standalone home directory
        get_home_directory()?
    } else if path_str.starts_with("~/") {
        // Handle home directory expansion
        expand_home_path(&path_str)?
    } else if path.is_absolute() {
        // Already absolute path
        path.to_path_buf()
    } else {
        // Relative path - resolve from current directory
        let current_dir = env::current_dir()
            .map_err(|e| CoreError::IoError(e))?;
        current_dir.join(path)
    };
    
    // Normalize the path to remove any . or .. components and clean up the path
    normalize_path_components(expanded)
}

/// Normalize path components to remove . and .. and clean up the path
fn normalize_path_components(path: PathBuf) -> CoreResult<PathBuf> {
    let mut components = Vec::new();
    
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {
                // Skip "." components
                continue;
            }
            std::path::Component::ParentDir => {
                // Handle ".." by popping the last component if possible
                if !components.is_empty() {
                    components.pop();
                }
            }
            other => {
                components.push(other);
            }
        }
    }
    
    // Reconstruct the path
    let mut result = PathBuf::new();
    for component in components {
        result.push(component);
    }
    
    // If we ended up with an empty path and the original was absolute, 
    // we should get the root path
    if result.as_os_str().is_empty() && path.is_absolute() {
        result.push("/");
    }
    
    Ok(result)
}

/// Expand a path starting with `~/` to use the actual home directory
fn expand_home_path(path: &str) -> CoreResult<PathBuf> {
    if !path.starts_with("~/") {
        return Err(CoreError::InvalidInput(
            "Path does not start with ~/".to_string()
        ));
    }
    
    let home_dir = get_home_directory()?;
    let rest_of_path = &path[2..]; // Remove "~/"
    
    if rest_of_path.is_empty() {
        Ok(home_dir)
    } else {
        Ok(home_dir.join(rest_of_path))
    }
}

/// Get the user's home directory
fn get_home_directory() -> CoreResult<PathBuf> {
    // Try HOME environment variable first (Unix-style)
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            return Ok(PathBuf::from(home));
        }
    }
    
    // Try USERPROFILE on Windows
    if let Ok(userprofile) = env::var("USERPROFILE") {
        if !userprofile.is_empty() {
            return Ok(PathBuf::from(userprofile));
        }
    }
    
    // Try HOMEDRIVE + HOMEPATH on Windows
    if let (Ok(drive), Ok(path)) = (env::var("HOMEDRIVE"), env::var("HOMEPATH")) {
        if !drive.is_empty() && !path.is_empty() {
            return Ok(PathBuf::from(format!("{}{}", drive, path)));
        }
    }
    
    Err(CoreError::ConfigError(
        "Could not determine home directory. Please set HOME environment variable.".to_string()
    ))
}

/// Validate that a directory path exists or can be created
pub fn validate_directory_path<P: AsRef<Path>>(path: P) -> CoreResult<PathBuf> {
    let expanded = expand_path(path)?;
    
    if expanded.exists() {
        if !expanded.is_dir() {
            return Err(CoreError::InvalidInput(
                format!("Path exists but is not a directory: {}", expanded.display())
            ));
        }
    } else {
        // Check if parent directory exists for creation
        if let Some(parent) = expanded.parent() {
            if !parent.exists() {
                return Err(CoreError::InvalidInput(
                    format!("Parent directory does not exist: {}", parent.display())
                ));
            }
        }
    }
    
    Ok(expanded)
}

/// Create a directory and any necessary parent directories
pub fn ensure_directory_exists<P: AsRef<Path>>(path: P) -> CoreResult<PathBuf> {
    let expanded = expand_path(path)?;
    
    if !expanded.exists() {
        std::fs::create_dir_all(&expanded)
            .map_err(|e| CoreError::IoError(e))?;
    } else if !expanded.is_dir() {
        return Err(CoreError::InvalidInput(
            format!("Path exists but is not a directory: {}", expanded.display())
        ));
    }
    
    Ok(expanded)
}

/// Convert a path to a canonical form for display/comparison
pub fn normalize_path<P: AsRef<Path>>(path: P) -> CoreResult<PathBuf> {
    let expanded = expand_path(path)?;
    
    // Try to canonicalize if the path exists
    if expanded.exists() {
        expanded.canonicalize()
            .map_err(|e| CoreError::IoError(e))
    } else {
        // Path doesn't exist, just return the expanded form
        Ok(expanded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    
    #[test]
    fn test_absolute_path() {
        let result = expand_path("/tmp/test").unwrap();
        assert_eq!(result, PathBuf::from("/tmp/test"));
    }
    
    #[test]
    fn test_relative_path() {
        let current_dir = env::current_dir().unwrap();
        let result = expand_path("./test").unwrap();
        assert_eq!(result, current_dir.join("test"));
        
        let result2 = expand_path("test").unwrap();
        assert_eq!(result2, current_dir.join("test"));
    }
    
    #[test] 
    fn test_home_path() {
        // Only test if HOME is available
        if let Ok(home) = env::var("HOME") {
            let result = expand_path("~/test").unwrap();
            assert_eq!(result, PathBuf::from(home.clone()).join("test"));
            
            let result2 = expand_path("~").unwrap();
            assert_eq!(result2, PathBuf::from(home));
        }
    }
    
    #[test]
    fn test_get_home_directory() {
        // Should work on most systems
        let result = get_home_directory();
        assert!(result.is_ok(), "Should be able to determine home directory");
        
        let home = result.unwrap();
        assert!(!home.as_os_str().is_empty());
    }
    
    #[test]
    fn test_current_directory() {
        let result = expand_path(".").unwrap();
        let current = env::current_dir().unwrap();
        assert_eq!(result, current);
    }
}