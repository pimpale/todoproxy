use std::{collections::HashMap, error::Error, fmt::Display};

use reqwest::{header::HeaderMap, Client};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct HabiticaClient {
    client: Client,
    author_id: String,
    script_name: String,
    api_user_id: String,
    api_key: String,
}

impl HabiticaClient {
    pub fn new(
        author_id: String,
        script_name: String,
        api_user_id: String,
        api_key: String,
    ) -> Self {
        HabiticaClient {
            author_id,
            script_name,
            api_user_id,
            api_key,
            client: Client::new(),
        }
    }

    fn construct_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-client",
            format!("{}-{}", self.author_id, self.script_name)
                .parse()
                .unwrap(),
        );
        headers.insert("x-api-user", self.api_user_id.parse().unwrap());
        headers.insert("x-api-key", self.api_key.parse().unwrap());
        headers
    }

    // gets all tasks of a user
    pub async fn get_user_tasks(&self) -> Result<Vec<Task>, HabiticaError> {
        #[derive(Deserialize)]
        struct TasksUserResp {
            success: bool,
            data: Vec<Task>,
        }
        let resp = self
            .client
            .get("https://habitica.com/api/v3/tasks/user")
            .headers(self.construct_headers())
            .send()
            .await?;
        match resp.status().as_u16() {
            200 => Ok(resp.json::<TasksUserResp>().await?.data),
            status => Err(HabiticaError::UnsuccessfulRequest(
                status,
                resp.text().await?,
            )),
        }
    }

    // mark a task as completed
    pub async fn score_task(
        &self,
        task_id: String,
        direction: Direction,
    ) -> Result<(), HabiticaError> {
        let resp = self
            .client
            .post(format!(
                "https://habitica.com/api/v3/tasks/{task_id}/score/{direction}"
            ))
            .headers(self.construct_headers())
            .header("content-length", 0)
            .send()
            .await?;
        match resp.status().as_u16() {
            200 => Ok(()),
            status => Err(HabiticaError::UnsuccessfulRequest(
                status,
                resp.text().await?,
            )),
        }
    }

    // update task
    pub async fn delete_task(&self, task_id: String) -> Result<(), HabiticaError> {
        let resp = self
            .client
            .delete(format!("https://habitica.com/api/v3/tasks/{task_id}"))
            .headers(self.construct_headers())
            .send()
            .await?;
        match resp.status().as_u16() {
            200 => Ok(()),
            status => Err(HabiticaError::UnsuccessfulRequest(
                status,
                resp.text().await?,
            )),
        }
    }

    // update task
    pub async fn update_task(&self, task_id: String, text: String) -> Result<(), HabiticaError> {
        #[derive(Serialize)]
        struct TaskUpdate {
            text: String,
        }
        let resp = self
            .client
            .put(format!("https://habitica.com/api/v3/tasks/{task_id}"))
            .headers(self.construct_headers())
            .json(&TaskUpdate { text })
            .send()
            .await?;
        match resp.status().as_u16() {
            200 => Ok(()),
            status => Err(HabiticaError::UnsuccessfulRequest(
                status,
                resp.text().await?,
            )),
        }
    }

    // move task to location
    pub async fn move_task(
        &self,
        task_id: String,
        location: i64,
    ) -> Result<Vec<String>, HabiticaError> {
        #[derive(Deserialize)]
        struct TaskMoveResp {
            success: bool,
            data: Vec<String>,
        }
        let resp = self
            .client
            .post(format!(
                "https://habitica.com/api/v3/tasks/{task_id}/move/to/{location}"
            ))
            .headers(self.construct_headers())
            .header("content-length", 0)
            .send()
            .await?;
        match resp.status().as_u16() {
            200 => Ok(resp.json::<TaskMoveResp>().await?.data),
            status => Err(HabiticaError::UnsuccessfulRequest(
                status,
                resp.text().await?,
            )),
        }
    }

    // get task data given the id
    pub async fn get_task(&self, task_id: String) -> Result<Task, HabiticaError> {
        #[derive(Deserialize)]
        struct TaskResp {
            success: bool,
            data: Task,
        }
        let resp = self
            .client
            .get(format!("https://habitica.com/api/v3/tasks/{task_id}"))
            .headers(self.construct_headers())
            .send()
            .await?;
        match resp.status().as_u16() {
            200 => Ok(resp.json::<TaskResp>().await?.data),
            status => Err(HabiticaError::UnsuccessfulRequest(
                status,
                resp.text().await?,
            )),
        }
    }

    // insert task
    pub async fn insert_todo(&self, alias: String, text: String) -> Result<Task, HabiticaError> {
        #[derive(Deserialize)]
        struct TaskResp {
            success: bool,
            data: Task,
        }
        #[derive(Serialize)]
        struct TaskReq {
            text: String,
            #[serde(rename = "type")]
            task_type: String,
            alias: String,
        }
        let resp = self
            .client
            .post(format!("https://habitica.com/api/v3/tasks/user"))
            .headers(self.construct_headers())
            .json(&TaskReq {
                text,
                task_type: String::from("todo"),
                alias,
            })
            .send()
            .await?;
        match resp.status().as_u16() {
            201 => Ok(resp.json::<TaskResp>().await?.data),
            status => Err(HabiticaError::UnsuccessfulRequest(
                status,
                resp.text().await?,
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Up,
    Down,
}

impl Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Direction::Up => write!(f, "up"),
            Direction::Down => write!(f, "down"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub _id: String,
    pub text: Option<String>,
    pub frequency: Option<String>,
    #[serde(rename = "type")]
    pub task_type: Option<String>,
    pub notes: Option<String>,
    pub repeat: Option<HashMap<String, bool>>,
    pub every_x: Option<i64>,
    pub next_due: Option<Vec<String>>,
    pub completed: Option<bool>,
    pub is_due: Option<bool>,
    pub checklist: Option<Vec<TaskCheckListItem>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TaskCheckListItem {
    pub completed: bool,
    pub text: String,
    pub id: String,
}

#[derive(Debug)]
pub enum HabiticaError {
    ReqwestError(reqwest::Error),
    UnsuccessfulRequest(u16, String),
}

impl Display for HabiticaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HabiticaError::ReqwestError(e) => write!(f, "habitica: {}", e),
            HabiticaError::UnsuccessfulRequest(status, text) => {
                write!(f, "habitica ({}): {}", status, text)
            }
        }
    }
}

impl Error for HabiticaError {}

impl From<reqwest::Error> for HabiticaError {
    fn from(value: reqwest::Error) -> Self {
        Self::ReqwestError(value)
    }
}
