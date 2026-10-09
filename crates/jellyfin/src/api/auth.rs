use reqwest::Method;
use serde_json::json;

use crate::{AuthResult, Client, PublicSystemInfo, QuickConnectState, Result, User};

impl Client {
    /// Unauthenticated probe: confirms the address is a Jellyfin server.
    pub async fn public_info(&self) -> Result<PublicSystemInfo> {
        self.get_json("System/Info/Public", &[]).await
    }

    /// Users the server lists on its sign-in screen (unauthenticated). Users
    /// hidden from the sign-in screen aren't included.
    pub async fn public_users(&self) -> Result<Vec<User>> {
        self.get_json("Users/Public", &[]).await
    }

    pub async fn me(&self) -> Result<User> {
        self.get_json("Users/Me", &[]).await
    }

    pub async fn authenticate_by_name(&self, username: &str, password: &str) -> Result<AuthResult> {
        let body = json!({ "Username": username, "Pw": password });
        self.post_json("Users/AuthenticateByName", &[], &body).await
    }

    pub async fn quick_connect_enabled(&self) -> Result<bool> {
        self.get_json("QuickConnect/Enabled", &[]).await
    }

    /// Starts a Quick Connect request; show `code` to the user and poll
    /// [`Client::quick_connect_state`] with `secret`.
    pub async fn quick_connect_initiate(&self) -> Result<QuickConnectState> {
        self.post_json("QuickConnect/Initiate", &[], &json!({}))
            .await
    }

    pub async fn quick_connect_state(&self, secret: &str) -> Result<QuickConnectState> {
        self.get_json("QuickConnect/Connect", &[("secret", secret.to_string())])
            .await
    }

    pub async fn authenticate_with_quick_connect(&self, secret: &str) -> Result<AuthResult> {
        let body = json!({ "Secret": secret });
        self.post_json("Users/AuthenticateWithQuickConnect", &[], &body)
            .await
    }

    /// Ends this device's session on the server (revokes the token).
    pub async fn logout(&self) -> Result<()> {
        self.send_empty(Method::POST, "Sessions/Logout", &[], None)
            .await
    }
}
