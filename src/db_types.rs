// a checkpoint may summarize the preceeding operations
// it may also be directly imported from habitica
#[derive(Clone, Debug)]
pub struct Checkpoint {
    pub checkpoint_id: i64,
    pub creation_time: i64,
    pub creator_user_id: i64,
    pub jsonval: String,
}

// the order of the operations in the database is the canonical order
#[derive(Clone, Debug)]
pub struct Operation {
    pub operation_id: i64,
    pub creation_time: i64,
    pub checkpoint_id: i64,
    pub jsonval: String,
}

