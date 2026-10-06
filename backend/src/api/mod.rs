pub mod auth_api;
pub mod assets;
pub mod classes;
pub mod logistics;
pub mod students;
pub mod transfers;

use crate::state::AppState;
use axum::Router;
use axum::routing::{get, patch, post};

pub fn router() -> Router<AppState> {
    Router::new()
        // 认证
        .route("/auth/login", post(auth_api::login))
        .route("/auth/logout", post(auth_api::logout))
        .route("/auth/me", get(auth_api::me))
        // 基础数据
        .route("/classes", get(classes::list_classes))
        .route("/models", get(classes::list_models))
        // 班级页面（待调换 / 已分配 / 损坏）
        .route("/classes/{class_id}/dashboard", get(classes::dashboard))
        .route("/classes/{class_id}/students", get(classes::list_students))
        // 学生
        .route("/students/{id}", get(students::get_student))
        .route("/students/{id}/height", patch(students::update_height))
        .route("/students/{id}/history", get(students::history))
        .route("/students/{id}/transfer-class", post(students::transfer_class))
        .route("/students/{id}/unassign", post(students::unassign))
        // 后勤：不匹配总览、建议（只读，不改归属）、原子领用
        .route("/logistics/overview", get(logistics::overview))
        .route("/logistics/mismatches", get(logistics::mismatches))
        .route("/logistics/suggestions", get(logistics::suggestions))
        .route("/allocations", post(logistics::create_allocation))
        // 资产 / 扫码
        .route("/assets", get(assets::list_assets))
        .route("/assets/{code}", get(assets::scan_asset))
        .route("/assets/{code}/history", get(assets::history))
        .route("/assets/{code}/damage", post(assets::mark_damaged))
        .route("/assets/{code}/repair", post(assets::repair))
        // 跨班调拨：发起 → 交出方确认 → 接收方确认
        .route("/transfers", post(transfers::create).get(transfers::list))
        .route("/transfers/{id}/release", post(transfers::release))
        .route("/transfers/{id}/confirm", post(transfers::confirm))
        .route("/transfers/{id}/reject", post(transfers::reject))
}
