use super::db_types::*;
use todoproxy_api::response::WebsocketServerUpdateMessage;
use tokio_postgres::GenericClient;

impl From<tokio_postgres::row::Row> for Operation {
    // select * from operation order only, otherwise it will fail
    fn from(row: tokio_postgres::Row) -> Operation {
        Operation {
            operation_id: row.get("operation_id"),
            creation_time: row.get("creation_time"),
            creator_user_id: row.get("creator_user_id"),
            jsonval: row.get("jsonval"),
        }
    }
}

pub async fn add(
    con: &mut impl GenericClient,
    creator_user_id: i64,
    op: WebsocketServerUpdateMessage,
) -> Result<Operation, tokio_postgres::Error> {
    let jsonval = serde_json::to_string(&op).unwrap();
    let row = con
        .query_one(
            "INSERT INTO
             operation(
                 creator_user_id,
                 jsonval
             )
             VALUES($1, $2)
             RETURNING operation_id, creation_time
            ",
            &[&creator_user_id, &jsonval],
        )
        .await?;

    // return operation
    Ok(Operation {
        operation_id: row.get(0),
        creation_time: row.get(1),
        creator_user_id,
        jsonval,
    })
}

pub async fn get_by_operation_id(
    con: &mut impl GenericClient,
    operation_id: i64,
) -> Result<Option<Operation>, tokio_postgres::Error> {
    let result = con
        .query_opt(
            "SELECT * FROM operation WHERE operation_id=$1",
            &[&operation_id],
        )
        .await?
        .map(|x| x.into());
    Ok(result)
}
