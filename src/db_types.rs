#[derive(Clone, Debug)]
pub struct HabiticaIntegration {
    pub habitica_integration_id: i64,
    pub creation_time: i64,
    pub creator_user_id: i64,
    pub user_id: String,
    pub api_key: String,
}
