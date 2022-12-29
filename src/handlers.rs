use super::response::*;

use actix_web::{web, Responder};


#[actix_web::post("/run_code")]
pub async fn run_code(
    req: web::Json<RunCodeRequest>,
    data: web::Data<PythonboxData>,
) -> Result<impl Responder, AppError> {
    // convert base64 tar gz into bytes
    let content = base64::decode(req.base_64_tar_gz.as_str()).map_err(|_| {
        error!(target: "pythonbox::run_code", "Invalid Base 64, refusing request");
        AppError::InvalidBase64
    })?;

    // max memory = 100MB
    let max_memory_usage = 100 * 0x100000;

    let resp = docker::run_code(
        content,
        req.max_time_s,
        max_memory_usage,
        data.image.clone(),
        data.docker.clone(),
    )
    .await?;

    return Ok(web::Json(RunCodeResponse {
        stdout: base64::encode(resp.stdout),
        stderr: base64::encode(resp.stderr),
        exit_code: resp.exit_code,
    }));
}
