//! Business logic handlers
//!
//! This module contains the business logic for handling API requests.

pub mod files;
pub mod local;
pub mod transfers;

use crate::error::{ApiError, ApiResult};

/// The ids of a bulk request, first occurrence kept; an empty list is refused before
/// anything reaches Put.io (feature 008, R8)
pub fn unique_ids(ids: Vec<i64>) -> ApiResult<Vec<i64>> {
    let mut seen = std::collections::HashSet::new();
    let ids: Vec<i64> = ids.into_iter().filter(|id| seen.insert(*id)).collect();
    if ids.is_empty() {
        return Err(ApiError::BadRequest("No ids given".to_string()));
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_ids_keeps_first_occurrence() {
        assert_eq!(unique_ids(vec![3, 1, 3, 2, 1]).unwrap(), vec![3, 1, 2]);
    }

    #[test]
    fn test_unique_ids_refuses_empty() {
        assert!(matches!(unique_ids(vec![]), Err(ApiError::BadRequest(_))));
    }
}
