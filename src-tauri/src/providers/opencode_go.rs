use super::{ProviderError, ProviderUsage};
use crate::{
    model::{Account, OpenCodeGoSecret, UsageWindow},
    state::AppState,
};
use chrono::{Duration, Utc};
use reqwest::{header::COOKIE, StatusCode};

const DASHBOARD_PREFIX: &str = "https://opencode.ai/workspace/";

#[derive(Clone, Copy, Debug)]
struct ParsedWindow {
    usage_percent: f64,
    reset_in_seconds: f64,
}

pub async fn refresh(
    app: &AppState,
    account: &Account,
    secret: &OpenCodeGoSecret,
) -> Result<ProviderUsage, ProviderError> {
    let workspace_id = secret.workspace_id.trim();
    let auth_cookie = normalize_cookie(&secret.auth_cookie);
    if workspace_id.is_empty() || auth_cookie.is_empty() {
        return Err(ProviderError::Auth);
    }

    let expected_path = format!("/workspace/{workspace_id}/go");
    let response = app
        .client
        .get(format!("{DASHBOARD_PREFIX}{workspace_id}/go"))
        .header("Accept", "text/html,application/xhtml+xml")
        .header(COOKIE, format!("auth={auth_cookie}"))
        .send()
        .await
        .map_err(|error| ProviderError::Transient(format!("OpenCode Go dashboard request failed: {error}")))?;
    let status = response.status();
    let final_path = response.url().path().to_string();
    let body = response.text().await.map_err(|error| {
        ProviderError::Transient(format!("Unable to read the OpenCode Go dashboard: {error}"))
    })?;

    if status == StatusCode::UNAUTHORIZED
        || status == StatusCode::FORBIDDEN
        || !final_path.starts_with(&expected_path)
        || looks_like_login(&body)
    {
        return Err(ProviderError::Auth);
    }
    if !status.is_success() {
        return Err(ProviderError::Transient(format!(
            "OpenCode Go dashboard returned {status}."
        )));
    }

    let mut rolling = parse_ssr_window(&body, "rollingUsage");
    let mut weekly = parse_ssr_window(&body, "weeklyUsage");
    let mut monthly = parse_ssr_window(&body, "monthlyUsage");
    if rolling.is_none() && weekly.is_none() && monthly.is_none() {
        let parsed = parse_data_slots(&body);
        rolling = parsed.0;
        weekly = parsed.1;
        monthly = parsed.2;
    }

    let mut windows = Vec::new();
    if let Some(window) = rolling {
        windows.push(normalize_window("five_hour", "5 hour", window, Some(18_000)));
    }
    if let Some(window) = weekly {
        windows.push(normalize_window("weekly", "Weekly", window, Some(604_800)));
    }
    if let Some(window) = monthly {
        windows.push(normalize_window("monthly", "Monthly", window, None));
    }
    if windows.is_empty() {
        return Err(ProviderError::Transient(
            "The OpenCode Go dashboard did not contain recognizable quota windows.".into(),
        ));
    }

    Ok(ProviderUsage {
        plan: Some("OpenCode Go".into()),
        email: account.email.clone(),
        provider_account_id: Some(workspace_id.to_string()),
        windows,
        credits_usd: None,
        unlimited_credits: false,
        source: "opencode_go_dashboard".into(),
    })
}

pub fn normalize_cookie(value: &str) -> String {
    let trimmed = value.trim();
    let trimmed = trimmed.strip_prefix("auth=").unwrap_or(trimmed);
    trimmed
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn looks_like_login(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("nav.login")
        || lower.contains(">login<")
        || lower.contains("sign in to opencode")
        || lower.contains("authorization required")
}

fn parse_ssr_window(body: &str, key: &str) -> Option<ParsedWindow> {
    let marker = format!("{key}:");
    let start = body.find(&marker)?;
    let segment = &body[start..body.len().min(start + 700)];
    let usage_percent = number_after(segment, "usagePercent:")?;
    let reset_in_seconds = number_after(segment, "resetInSec:")?;
    Some(ParsedWindow {
        usage_percent,
        reset_in_seconds,
    })
}

fn number_after(value: &str, marker: &str) -> Option<f64> {
    let start = value.find(marker)? + marker.len();
    let tail = value[start..].trim_start();
    let number = tail
        .chars()
        .take_while(|character| {
            character.is_ascii_digit() || matches!(character, '-' | '+' | '.' | 'e' | 'E')
        })
        .collect::<String>();
    if number.is_empty() {
        None
    } else {
        number.parse().ok()
    }
}

fn parse_data_slots(body: &str) -> (Option<ParsedWindow>, Option<ParsedWindow>, Option<ParsedWindow>) {
    let mut rolling = None;
    let mut weekly = None;
    let mut monthly = None;
    for segment in body.split("data-slot=\"usage-item\"").skip(1) {
        let block = &segment[..segment.len().min(1400)];
        let Some(label) = text_after(block, "data-slot=\"usage-label\">") else {
            continue;
        };
        let Some(usage_text) = text_after(block, "data-slot=\"usage-value\">") else {
            continue;
        };
        let Some(usage_percent) = first_number(&usage_text) else {
            continue;
        };
        let reset_text = text_after(block, "data-slot=\"reset-time\">")
            .or_else(|| text_after(block, "data-slot=\"reset-now\">"))
            .unwrap_or_default();
        let reset_in_seconds = parse_human_duration(&strip_markup(&reset_text)).unwrap_or_default();
        let parsed = ParsedWindow {
            usage_percent,
            reset_in_seconds,
        };
        let lower = label.to_ascii_lowercase();
        if lower.contains("rolling") || lower.contains("5 hour") {
            rolling = Some(parsed);
        } else if lower.contains("weekly") {
            weekly = Some(parsed);
        } else if lower.contains("monthly") {
            monthly = Some(parsed);
        }
    }
    (rolling, weekly, monthly)
}

fn text_after(value: &str, marker: &str) -> Option<String> {
    let start = value.find(marker)? + marker.len();
    let tail = &value[start..];
    let end = tail.find('<').unwrap_or(tail.len());
    Some(tail[..end].to_string())
}

fn first_number(value: &str) -> Option<f64> {
    let start = value.find(|character: char| character.is_ascii_digit() || character == '-')?;
    let number = value[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit() || matches!(character, '-' | '.'))
        .collect::<String>();
    number.parse().ok()
}

fn strip_markup(value: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(character),
            _ => {}
        }
    }
    result
        .replace("Resets in", "")
        .replace("resets in", "")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

fn parse_human_duration(value: &str) -> Option<f64> {
    let lower = value.to_ascii_lowercase();
    if lower.trim().is_empty() || lower.contains("now") {
        return Some(0.0);
    }
    let mut total = 0.0;
    let mut found = false;
    let words = lower.split_whitespace().collect::<Vec<_>>();
    for pair in words.windows(2) {
        let Ok(amount) = pair[0].trim_matches(|character: char| !character.is_ascii_digit() && character != '.').parse::<f64>() else {
            continue;
        };
        let unit = pair[1].trim_matches(|character: char| !character.is_ascii_alphabetic());
        let multiplier = if unit.starts_with("day") {
            86_400.0
        } else if unit.starts_with("hour") || unit == "hr" || unit == "hrs" {
            3_600.0
        } else if unit.starts_with("minute") || unit == "min" || unit == "mins" {
            60.0
        } else if unit.starts_with("second") || unit == "sec" || unit == "secs" {
            1.0
        } else {
            continue;
        };
        total += amount * multiplier;
        found = true;
    }
    found.then_some(total)
}

fn normalize_window(
    id: &str,
    label: &str,
    raw: ParsedWindow,
    window_seconds: Option<u64>,
) -> UsageWindow {
    let used = raw.usage_percent.clamp(0.0, 100.0);
    let reset_in_seconds = raw.reset_in_seconds.max(0.0).round() as i64;
    UsageWindow {
        id: id.into(),
        label: label.into(),
        used_percent: Some(used),
        remaining_percent: Some((100.0 - used).max(0.0)),
        resets_at: Some((Utc::now() + Duration::seconds(reset_in_seconds)).to_rfc3339()),
        window_seconds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssr_usage_in_either_field_order() {
        let html = r#"rollingUsage:$R[1]={resetInSec:1800,usagePercent:25}"#;
        let parsed = parse_ssr_window(html, "rollingUsage").unwrap();
        assert_eq!(parsed.usage_percent, 25.0);
        assert_eq!(parsed.reset_in_seconds, 1800.0);
    }

    #[test]
    fn normalizes_pasted_cookie_header() {
        assert_eq!(normalize_cookie("auth=abc123; Path=/"), "abc123");
    }

    #[test]
    fn parses_human_reset_duration() {
        assert_eq!(parse_human_duration("1 hour 30 minutes"), Some(5400.0));
    }
}
