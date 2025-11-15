//! Account information and verification API endpoints

use super::client::ApiClient;
use super::models::{AccountInfo, AccountInfoResponse, ApiResponse};
use crate::error::CoreResult;

impl ApiClient {
    /// Get account information
    ///
    /// Retrieves account details including username, disk usage, and quota.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_core::api::ApiClient;
    /// # async fn example() {
    /// let client = ApiClient::new("my_token".to_string());
    /// let info = client.get_account_info().await.unwrap();
    /// println!("Username: {}", info.username);
    /// println!("Disk used: {} bytes", info.disk.used);
    /// # }
    /// ```
    pub async fn get_account_info(&self) -> CoreResult<AccountInfo> {
        let response: ApiResponse<AccountInfoResponse> = self.get("/account/info").await?;
        Ok(response.data.info)
    }

    /// Verify OAuth token
    ///
    /// Verifies that the OAuth token is valid by attempting to fetch account info.
    /// Returns true if the token is valid, false otherwise.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_core::api::ApiClient;
    /// # async fn example() {
    /// let client = ApiClient::new("my_token".to_string());
    /// if client.verify_token().await {
    ///     println!("Token is valid");
    /// } else {
    ///     println!("Token is invalid");
    /// }
    /// # }
    /// ```
    pub async fn verify_token(&self) -> bool {
        self.get_account_info().await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_get_account_info() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/account/info")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "info": {
                        "user_id": 12345,
                        "username": "testuser",
                        "mail": "test@example.com",
                        "account_active": true,
                        "disk": {
                            "size": 10737418240,
                            "used": 5368709120,
                            "avail": 5368709120
                        },
                        "can_create_sub_account": false
                    }
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.get_account_info().await;

        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.user_id, 12345);
        assert_eq!(info.username, "testuser");
        assert_eq!(info.mail, "test@example.com");
        assert!(info.account_active);
        assert_eq!(info.disk.size, 10737418240);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_verify_token_valid() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/account/info")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "info": {
                        "user_id": 12345,
                        "username": "testuser",
                        "mail": "test@example.com",
                        "account_active": true,
                        "disk": {
                            "size": 10737418240,
                            "used": 5368709120,
                            "avail": 5368709120
                        },
                        "can_create_sub_account": false
                    }
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.verify_token().await;

        assert!(result);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_verify_token_invalid() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/account/info")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error": "unauthorized"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("bad_token".to_string(), server.url());
        let result = client.verify_token().await;

        assert!(!result);
        mock.assert_async().await;
    }
}
