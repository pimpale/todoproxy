use std::collections::VecDeque;

use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, FromPrimitive)]
pub enum TaskStatus {
    Obsoleted,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveTask {
    pub id: String,
    pub value: String,
    pub managed: Option<String>,
    pub deadline: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedTask {
    pub id: String,
    pub value: String,
    pub managed: Option<String>,
    pub status: TaskStatus,
    pub deadline: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateSnapshot {
    pub live: VecDeque<LiveTask>,
    pub finished: VecDeque<FinishedTask>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WebsocketOpKind {
    OverwriteState(StateSnapshot),
    InsLiveTask { id: String, value: String, deadline: Option<u64> },
    RestoreFinishedTask { id: String },
    EditLiveTask { id: String, value: String, deadline: Option<u64> },
    DelLiveTask { id: String },
    MvLiveTask { id_del: String, id_ins: String },
    RevLiveTask { id1: String, id2: String },
    FinishLiveTask { id: String, status: TaskStatus },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WebsocketOp {
    pub alleged_time: i64,
    pub kind: WebsocketOpKind,
}

pub mod request {
    use serde::{Deserialize, Serialize};

    use super::TaskStatus;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct WebsocketInitMessage {
        pub api_key: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ManagedTaskNew {
        pub id: String,
        pub value: String,
        pub source: String,
        pub deadline: Option<u64>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct ManagedTaskFinish {
        pub id: String,
        pub status: TaskStatus,
    }
}
pub mod response {
    use derive_more::Display;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize, Display)]
    #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
    pub enum AppError {
        DecodeError,
        InternalServerError,
        Unauthorized,
        BadRequest,
        NotFound,
        Unknown,
    }

    impl std::error::Error for AppError {}

    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Info {
        pub service: String,
        pub version_major: i64,
        pub version_minor: i64,
        pub version_rev: i64,
        pub app_pub_origin: String,
        pub auth_pub_api_href: String,
        pub auth_authenticator_href: String,
    }
}
