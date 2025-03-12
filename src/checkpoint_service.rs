use super::db_types::*;
use crate::api::StateSnapshot;
use tokio_postgres::GenericClient;

impl From<tokio_postgres::row::Row> for Checkpoint {
    // select * from checkpoint order only, otherwise it will fail
    fn from(row: tokio_postgres::Row) -> Checkpoint {
        Checkpoint {
            checkpoint_id: row.get("checkpoint_id"),
            creation_time: row.get("creation_time"),
            creator_user_id: row.get("creator_user_id"),
            jsonval: row.get("jsonval"),
        }
    }
}

pub async fn add(
    con: &mut impl GenericClient,
    creator_user_id: i64,
    checkpoint: StateSnapshot,
) -> Result<Checkpoint, tokio_postgres::Error> {
    let jsonval = serde_json::to_string(&checkpoint).unwrap();
    let row = con
        .query_one(
            "INSERT INTO
             checkpoint(
                 creator_user_id,
                 jsonval
             )
             VALUES($1, $2)
             RETURNING checkpoint_id, creation_time
            ",
            &[&creator_user_id, &jsonval],
        )
        .await?;

    // return checkpoint
    Ok(Checkpoint {
        checkpoint_id: row.get(0),
        creation_time: row.get(1),
        creator_user_id,
        jsonval,
    })
}

pub async fn get_by_checkpoint_id(
    con: &mut impl GenericClient,
    checkpoint_id: i64,
) -> Result<Option<Checkpoint>, tokio_postgres::Error> {
    let result = con
        .query_opt(
            "SELECT * FROM checkpoint WHERE checkpoint_id=$1",
            &[&checkpoint_id],
        )
        .await?
        .map(|x| x.into());
    Ok(result)
}

pub async fn get_recent_by_user_id(
    con: &mut impl GenericClient,
    creator_user_id: i64,
) -> Result<Option<Checkpoint>, tokio_postgres::Error> {
    let result = con
        .query_opt(
            "SELECT * FROM recent_checkpoint_by_user_id WHERE creator_user_id=$1",
            &[&creator_user_id],
        )
        .await?
        .map(|x| x.into());
    Ok(result)
}
