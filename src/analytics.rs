//! 用量与费用的统计计算。
//!
//! 从界面层搬出来的：原先在 `ui/analytics.rs` 里，每次弹窗渲染都会遍历全部会话和消息。
//! 这里只做计算，配色、布局、图表这些展示相关的东西留在 `ui/analytics.rs`。
//!
//! 为了守住分层，这里不认 `AppState`，也不查 models.dev：会话从参数传进来，
//! 单价由调用方用 `cost_of` 注入。

use std::collections::HashMap;

use chrono::{Duration, Local, NaiveDate};

use crate::model::ChatSession;

/// 统计看板的时间范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeRange {
    Days7,
    Days30,
    All,
}

/// 统计看板当前显示的分页。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalyticsTab {
    Overview,
    Models,
}

#[derive(Clone, Debug)]
pub struct DayStat {
    pub date_str: String,  // "09-26"
    pub full_date: String, // "2026-09-26"
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub total_tokens: usize,
    pub cost_usd: f64,
    pub cost_known: bool,
}

#[derive(Clone, Debug)]
pub struct ModelStat {
    pub model_id: String,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub total_tokens: usize,
    pub cost_usd: f64,
    pub cost_known: bool,
    pub percentage: f32,
}

pub struct AnalyticsSummary {
    pub total_input: usize,
    pub total_output: usize,
    pub total_tokens: usize,
    pub total_cost_usd: f64,
    pub unknown_costs: usize,
    pub total_messages: usize,
    pub daily: Vec<DayStat>,
    pub models: Vec<ModelStat>,
}

/// 汇总一段时间内的用量与费用。
///
/// `default_model` 是消息和会话都没记模型时的兜底；`cost_of` 按 (模型, 输入 token, 输出 token)
/// 返回美元单价，由调用方注入，这样这里不必依赖 models.dev。
pub fn collect_stats(
    sessions: &[ChatSession],
    default_model: &str,
    cost_of: &dyn Fn(&str, usize, usize) -> Option<f64>,
    range: TimeRange,
) -> AnalyticsSummary {
    let now = Local::now().date_naive();
    let min_date = match range {
        TimeRange::Days7 => Some(now - Duration::days(6)),
        TimeRange::Days30 => Some(now - Duration::days(29)),
        TimeRange::All => None,
    };

    let mut total_input = 0usize;
    let mut total_output = 0usize;
    let mut total_messages = 0usize;
    let mut total_cost_usd = 0.0f64;
    let mut unknown_costs = 0usize;

    let mut daily_map: HashMap<String, (usize, usize, f64, bool)> = HashMap::new();
    let mut model_map: HashMap<String, (usize, usize, f64, bool)> = HashMap::new();

    for session in sessions {
        for msg in &session.messages {
            if msg.role != "assistant" {
                continue;
            }

            let date = if msg.created_at.len() >= 10 && msg.created_at.contains('-') {
                NaiveDate::parse_from_str(&msg.created_at[..10], "%Y-%m-%d").ok()
            } else if session.updated_at.len() >= 10 && session.updated_at.contains('-') {
                NaiveDate::parse_from_str(&session.updated_at[..10], "%Y-%m-%d").ok()
            } else if session.created_at.len() >= 10 && session.created_at.contains('-') {
                NaiveDate::parse_from_str(&session.created_at[..10], "%Y-%m-%d").ok()
            } else {
                None
            }
            .unwrap_or(now);

            if let Some(min) = min_date
                && date < min
            {
                continue;
            }

            let date_prefix = date.format("%Y-%m-%d").to_string();

            // 对比请求各自计费；采用某个结果后父消息会复制该 variant 的用量，不能重复计入。
            let turns = std::iter::once((msg.model.as_str(), msg.prompt_tokens, msg.completion_tokens))
                .filter(|_| msg.variants.is_empty())
                .chain(
                    msg.variants
                        .iter()
                        .map(|v| (v.model.as_str(), v.prompt_tokens, v.completion_tokens)),
                );
            for (model_id, in_tok, out_tok) in turns {
                if in_tok == 0 && out_tok == 0 {
                    continue;
                }
                let model = if !model_id.is_empty() {
                    model_id.to_string()
                } else if !session.model.is_empty() && session.model != "default" {
                    session.model.clone()
                } else {
                    default_model.to_string()
                };

                let cost = cost_of(&model, in_tok, out_tok);
                let cost_known = cost.is_some();
                if !cost_known {
                    unknown_costs += 1;
                }
                let cost_usd = cost.unwrap_or_default();

                total_input += in_tok;
                total_output += out_tok;
                total_messages += 1;
                total_cost_usd += cost_usd;

                let day_entry = daily_map.entry(date_prefix.clone()).or_insert((0, 0, 0.0, true));
                day_entry.0 += in_tok;
                day_entry.1 += out_tok;
                day_entry.2 += cost_usd;
                day_entry.3 &= cost_known;

                let model_entry = model_map.entry(model).or_insert((0, 0, 0.0, true));
                model_entry.0 += in_tok;
                model_entry.1 += out_tok;
                model_entry.2 += cost_usd;
                model_entry.3 &= cost_known;
            }
        }
    }

    let total_tokens = total_input + total_output;

    let days_count = match range {
        TimeRange::Days7 => 7,
        TimeRange::Days30 => 30,
        TimeRange::All => {
            if let Some(min) = daily_map.keys().min() {
                if let Ok(earliest) = NaiveDate::parse_from_str(min, "%Y-%m-%d") {
                    ((now - earliest).num_days() + 1).clamp(7, 60)
                } else {
                    14
                }
            } else {
                14
            }
        }
    };

    let mut daily = Vec::new();
    for i in (0..days_count).rev() {
        let d = now - Duration::days(i);
        let full_date = d.format("%Y-%m-%d").to_string();
        let date_str = d.format("%m/%d").to_string();
        let (in_t, out_t, c_usd, cost_known) = daily_map.get(&full_date).copied().unwrap_or((0, 0, 0.0, true));
        daily.push(DayStat {
            date_str,
            full_date,
            input_tokens: in_t,
            output_tokens: out_t,
            total_tokens: in_t + out_t,
            cost_usd: c_usd,
            cost_known,
        });
    }

    let mut model_list: Vec<(String, usize, usize, f64, bool)> = model_map
        .into_iter()
        .map(|(m, (i, o, c, known))| (m, i, o, c, known))
        .collect();
    model_list.sort_by_key(|b| std::cmp::Reverse(b.1 + b.2));

    let mut models = Vec::new();
    for (model_id, in_t, out_t, c_usd, cost_known) in model_list.into_iter() {
        let m_tot = in_t + out_t;
        let percentage = if total_tokens > 0 {
            (m_tot as f32 / total_tokens as f32) * 100.0
        } else {
            0.0
        };
        models.push(ModelStat {
            model_id,
            input_tokens: in_t,
            output_tokens: out_t,
            total_tokens: m_tot,
            cost_usd: c_usd,
            cost_known,
            percentage,
        });
    }

    AnalyticsSummary {
        total_input,
        total_output,
        total_tokens,
        total_cost_usd,
        unknown_costs,
        total_messages,
        daily,
        models,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChatMessage, MessageVariant};
    use serde_json::json;

    #[test]
    fn comparison_counts_each_request_once_even_after_adoption() {
        let mut session = ChatSession::new("Compare".into(), "Work".into(), "opus".into(), "p".into());
        let mut message = ChatMessage::new_assistant();
        message.variants = vec![
            serde_json::from_value::<MessageVariant>(json!({"id":"a", "provider_id":"p", "model":"opus", "content":"A", "prompt_tokens":31, "completion_tokens":62})).unwrap(),
            serde_json::from_value::<MessageVariant>(json!({"id":"b", "provider_id":"p", "model":"sonnet", "content":"B", "prompt_tokens":31, "completion_tokens":71})).unwrap(),
        ];
        session.messages.push(message);
        let cost = |model: &str, input: usize, output: usize| {
            if model == "opus" {
                Some((input + output) as f64 / 1000.0)
            } else {
                None
            }
        };

        for adopted in [false, true] {
            if adopted {
                session.messages[0].model = "opus".into();
                session.messages[0].prompt_tokens = 31;
                session.messages[0].completion_tokens = 62;
            }
            let result = collect_stats(std::slice::from_ref(&session), "fallback", &cost, TimeRange::Days7);
            assert_eq!(
                (result.total_input, result.total_output, result.total_messages),
                (62, 133, 2)
            );
            assert_eq!(result.unknown_costs, 1);
            assert_eq!(result.models.len(), 2);
            assert_eq!(result.daily.last().map(|day| day.total_tokens), Some(195));
            assert!((result.total_cost_usd - 0.093).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn ordinary_reply_still_counts_once() {
        let mut session = ChatSession::new("Chat".into(), "Work".into(), "opus".into(), "p".into());
        let mut message = ChatMessage::new_assistant();
        message.prompt_tokens = 8;
        message.completion_tokens = 5;
        session.messages.push(message);
        let result = collect_stats(&[session], "fallback", &|_, _, _| Some(0.0), TimeRange::Days7);
        assert_eq!(
            (result.total_input, result.total_output, result.total_messages),
            (8, 5, 1)
        );
        assert_eq!(result.models[0].model_id, "opus");
    }
}
