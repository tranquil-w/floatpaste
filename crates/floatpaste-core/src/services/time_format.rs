//! 相对时间格式化：速贴面板、tooltip、搜索窗口共用的"刚刚 / N 分钟前 / 昨天"口径，
//! 与前端 `shared/utils/time.ts` 逐分支对齐；超过一周显示 "M月D日 HH:MM"（24 小时制）。

use chrono::{DateTime, Datelike, Local, Timelike};

pub fn format_relative_time(value: &str) -> String {
    let Some(instant) = parse_local(value) else {
        return value.to_string();
    };

    let now = Local::now();
    let diff_ms = now.signed_duration_since(instant).num_milliseconds();
    if diff_ms < 0 {
        return format_absolute(&instant);
    }

    let diff_sec = diff_ms / 1000;
    let diff_min = diff_sec / 60;
    let diff_hour = diff_min / 60;
    let diff_day = diff_hour / 24;

    if diff_sec < 60 {
        "刚刚".to_string()
    } else if diff_min < 60 {
        format!("{diff_min} 分钟前")
    } else if diff_hour < 24 {
        format!("{diff_hour} 小时前")
    } else if diff_day == 1 {
        "昨天".to_string()
    } else if diff_day < 7 {
        format!("{diff_day} 天前")
    } else {
        format_absolute(&instant)
    }
}

/// 空值场景（lastUsedAt 为 null 时前端显示"未使用"）
pub fn format_relative_time_or_unused(value: Option<&str>) -> String {
    value.map_or_else(|| "未使用".to_string(), format_relative_time)
}

fn format_absolute(instant: &DateTime<Local>) -> String {
    format!(
        "{}月{}日 {:02}:{:02}",
        instant.month(),
        instant.day(),
        instant.hour(),
        instant.minute()
    )
}

fn parse_local(value: &str) -> Option<DateTime<Local>> {
    let parsed = DateTime::parse_from_rfc3339(value).ok()?;
    Some(parsed.with_timezone(&Local))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn rfc_from(instant: DateTime<Local>) -> String {
        instant.to_rfc3339()
    }

    #[test]
    fn recent_buckets() {
        let now = Local::now();
        assert_eq!(format_relative_time(&rfc_from(now)), "刚刚");
        assert_eq!(
            format_relative_time(&rfc_from(now - Duration::seconds(30))),
            "刚刚"
        );
        assert_eq!(
            format_relative_time(&rfc_from(now - Duration::minutes(5))),
            "5 分钟前"
        );
        assert_eq!(
            format_relative_time(&rfc_from(now - Duration::hours(3))),
            "3 小时前"
        );
        assert_eq!(
            format_relative_time(&rfc_from(now - Duration::hours(26))),
            "昨天"
        );
        assert_eq!(
            format_relative_time(&rfc_from(now - Duration::days(3))),
            "3 天前"
        );
    }

    #[test]
    fn over_a_week_falls_back_to_absolute_date() {
        let value = format_relative_time(&rfc_from(Local::now() - Duration::days(10)));
        assert!(value.contains("月"), "{value}");
        assert!(value.contains(':'), "{value}");
    }

    #[test]
    fn invalid_value_is_returned_as_is() {
        assert_eq!(format_relative_time("not-a-date"), "not-a-date");
    }

    #[test]
    fn none_shows_unused() {
        assert_eq!(format_relative_time_or_unused(None), "未使用");
    }
}
