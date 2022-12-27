use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn current_time_millis() -> i64 {
    let since_the_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards");

    since_the_epoch.as_millis() as i64
}
