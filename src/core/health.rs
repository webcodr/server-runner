use anyhow::bail;
use std::time::Duration;

use crate::core::state::ServerStatus;

/// Perform a single readiness probe against `url`.
/// Returns `Waiting` if the server is not yet up, `Running` on HTTP 2xx.
/// Returns `Err` only for non-transient errors.
pub async fn check(name: &str, url: &str, timeout_secs: u64) -> anyhow::Result<ServerStatus> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    match client.get(url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                Ok(ServerStatus::Running)
            } else {
                Ok(ServerStatus::Waiting)
            }
        }
        Err(error) => {
            if error.is_connect() || error.is_timeout() {
                Ok(ServerStatus::Waiting)
            } else {
                bail!("Could not connect to server {} on url {}", name, url);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connection_refused_is_waiting() {
        // Port 1 is privileged and always refused on Linux; gives a fast connect error.
        let status = check("Test", "http://127.0.0.1:1", 1).await.unwrap();
        assert_eq!(status, ServerStatus::Waiting);
    }
}
