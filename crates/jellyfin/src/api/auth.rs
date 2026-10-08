use crate::{Client, PublicSystemInfo, Result, User};

impl Client {
    /// Unauthenticated probe: confirms the address is a Jellyfin server.
    pub async fn public_info(&self) -> Result<PublicSystemInfo> {
        self.get_json("System/Info/Public", &[]).await
    }

    pub async fn me(&self) -> Result<User> {
        self.get_json("Users/Me", &[]).await
    }
}
