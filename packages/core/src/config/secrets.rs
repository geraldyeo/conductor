use super::schema::ResolvedSecrets;

pub fn load_secrets() -> ResolvedSecrets {
    ResolvedSecrets {
        linear_api_key: std::env::var("LINEAR_API_KEY").ok(),
        slack_webhook_url: std::env::var("SLACK_WEBHOOK_URL").ok(),
        composio_api_key: std::env::var("COMPOSIO_API_KEY").ok(),
    }
}
