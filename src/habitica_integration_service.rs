use super::db_types::*;
use todoproxy_api::StateSnapshot;
use tokio_postgres::GenericClient;

impl From<tokio_postgres::row::Row> for HabiticaIntegration {
    // select * from habitica_integration order only, otherwise it will fail
    fn from(row: tokio_postgres::Row) -> HabiticaIntegration {
        HabiticaIntegration {
            habitica_integration_id: row.get("habitica_integration_id"),
            creation_time: row.get("creation_time"),
            creator_user_id: row.get("creator_user_id"),
            user_id: row.get("user_id"),
            api_key: row.get("api_key"),
        }
    }
}

pub async fn add(
    con: &mut impl GenericClient,
    creator_user_id: i64,
    user_id: String,
    api_key: String,
) -> Result<HabiticaIntegration, tokio_postgres::Error> {
    let jsonval = serde_json::to_string(&habitica_integration).unwrap();
    let row = con
        .query_one(
            "INSERT INTO
             habitica_integration(
                 creator_user_id,
                 user_id,
                 api_key
             )
             VALUES($1, $2, $3)
             RETURNING habitica_integration_id, creation_time
            ",
            &[&creator_user_id, &user_id, &api_key],
        )
        .await?;

    // return habitica_integration
    Ok(HabiticaIntegration {
        habitica_integration_id: row.get(0),
        creation_time: row.get(1),
        creator_user_id,
        user_id,
        api_key,
    })
}

pub async fn get_by_habitica_integration_id(
    con: &mut impl GenericClient,
    habitica_integration_id: i64,
) -> Result<Option<HabiticaIntegration>, tokio_postgres::Error> {
    let result = con
        .query_opt(
            "SELECT * FROM habitica_integration WHERE habitica_integration_id=$1",
            &[&habitica_integration_id],
        )
        .await?
        .map(|x| x.into());
    Ok(result)
}

pub async fn get_recent_by_user_id(
    con: &mut impl GenericClient,
    creator_user_id: i64,
) -> Result<Option<HabiticaIntegration>, tokio_postgres::Error> {
    let result = con
        .query_opt(
            "SELECT * FROM recent_habitica_integration_by_user_id WHERE creator_user_id=$1",
            &[&creator_user_id],
        )
        .await?
        .map(|x| x.into());
    Ok(result)
}
