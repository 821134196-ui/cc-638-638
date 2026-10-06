use serde::Serialize;

/// 身高 → 推荐型号（型号表与数据库 desk_models 一致）。
/// 纯函数，便于测试；区间为闭区间，按身高从矮到高匹配第一个命中的型号。
pub fn recommend_model(height_cm: i64) -> &'static str {
    const BANDS: &[(i64, &str)] = &[
        (112, "10号"),
        (119, "9号"),
        (127, "8号"),
        (134, "7号"),
        (142, "6号"),
        (149, "5号"),
        (157, "4号"),
        (164, "3号"),
        (172, "2号"),
        (179, "1号"),
        (220, "0号"),
    ];
    for &(max, code) in BANDS {
        if height_cm <= max {
            return code;
        }
    }
    "0号"
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// "too_large" 现有桌椅型号偏大（桌椅太高）/ "too_small" 偏小 / "unassigned" 没有桌椅
    pub kind: String,
    pub current_model: Option<String>,
    pub needed_model: String,
    pub message: String,
}

/// 判断学生身高与现占用桌椅是否匹配。None 表示匹配。
pub fn evaluate(height_cm: i64, current_model: Option<&str>) -> Option<Mismatch> {
    let needed = recommend_model(height_cm);
    match current_model {
        None => Some(Mismatch {
            kind: "unassigned".into(),
            current_model: None,
            needed_model: needed.into(),
            message: format!("学生没有可用桌椅，需配备 {needed} 号桌椅"),
        }),
        Some(cur) if cur == needed => None,
        Some(cur) => {
            // 型号编号数字越大桌椅越小：cur 编号 > needed 编号 → 现有桌椅偏小
            let cur_no: u32 = cur.trim_end_matches('号').parse().unwrap_or(0);
            let need_no: u32 = needed.trim_end_matches('号').parse().unwrap_or(0);
            let (kind, msg) = if cur_no > need_no {
                ("too_small", format!("现有 {cur} 桌椅偏小（矮），身高 {height_cm}cm 应配 {needed}"))
            } else {
                ("too_large", format!("现有 {cur} 桌椅偏大（高），身高 {height_cm}cm 应配 {needed}"))
            };
            Some(Mismatch {
                kind: kind.into(),
                current_model: Some(cur.into()),
                needed_model: needed.into(),
                message: msg.into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bands() {
        assert_eq!(recommend_model(132), "7号");
        assert_eq!(recommend_model(145), "5号");
        assert_eq!(recommend_model(152), "4号");
        assert_eq!(recommend_model(160), "3号");
        assert_eq!(recommend_model(112), "10号");
        assert_eq!(recommend_model(200), "0号");
    }

    #[test]
    fn cases() {
        assert!(evaluate(145, Some("5号")).is_none());
        let m = evaluate(132, Some("6号")).unwrap();
        assert_eq!(m.kind, "too_large");
        let m = evaluate(152, Some("7号")).unwrap();
        assert_eq!(m.kind, "too_small");
        let m = evaluate(138, None).unwrap();
        assert_eq!(m.kind, "unassigned");
        assert_eq!(m.needed_model, "6号");
    }
}
