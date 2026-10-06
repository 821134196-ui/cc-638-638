use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct DeskModel {
    pub code: String,
    pub name: String,
    pub height_min: i64,
    pub height_max: i64,
    pub desk_cm: i64,
    pub chair_cm: i64,
}

/// 按身高推荐型号；超出 110~175 区间时向边界型号靠拢
pub fn desired_model(height_cm: i64) -> &'static str {
    const BANDS: [(i64, i64, &str); 6] = [
        (110, 119, "0"),
        (120, 129, "1"),
        (130, 139, "2"),
        (140, 149, "3"),
        (150, 159, "4"),
        (160, 175, "5"),
    ];
    if height_cm < 120 {
        return "0";
    }
    for (min, max, code) in BANDS {
        if height_cm >= min && height_cm <= max {
            return code;
        }
    }
    "5"
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Class {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Student {
    pub id: i64,
    pub name: String,
    pub class_id: i64,
    pub height_cm: Option<i64>,
    pub desired_model: Option<String>,
    pub active: i64,
    pub updated_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Asset {
    pub code: String,
    pub model: String,
    pub class_id: i64,
    pub status: String, // ok | damaged
    pub note: String,
    pub updated_at: String,
}

/// 班级看板用：资产 + 当前使用学生 + 当前调拨状态
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AssetView {
    pub code: String,
    pub model: String,
    pub class_id: i64,
    pub status: String,
    pub note: String,
    pub occupant_student_id: Option<i64>,
    pub occupant_name: Option<String>,
    pub pending_transfer_to_class: Option<i64>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AssignmentView {
    pub id: i64,
    pub asset_code: String,
    pub student_id: i64,
    pub status: String,
    pub reason: String,
    pub created_at: String,
    pub released_at: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SuggestionView {
    pub id: i64,
    pub student_id: i64,
    pub student_name: String,
    pub class_id: i64,
    pub height_cm: Option<i64>,
    pub model: String,
    pub current_asset: Option<String>,
    pub current_model: Option<String>,
    pub reason: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TransferView {
    pub id: i64,
    pub asset_code: String,
    pub from_class_id: i64,
    pub to_class_id: i64,
    pub requested_by: i64,
    pub from_confirmed_by: Option<i64>,
    pub to_confirmed_by: Option<i64>,
    pub status: String,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub cancel_reason: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct TrailEntry {
    pub id: i64,
    pub asset_code: String,
    pub event: String,
    pub detail: String,
    pub actor_id: Option<i64>,
    pub created_at: String,
}
