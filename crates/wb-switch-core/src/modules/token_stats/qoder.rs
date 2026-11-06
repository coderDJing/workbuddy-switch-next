//! Qoder persists desktop/CLI sessions together under ~/.qoder[-cn]/projects.
//! Only usage metadata leaves this reader. The current client writes zero token
//! placeholders even for paid requests; these are unavailable, never zero usage.
use super::*;
use crate::modules::qoder::{QoderRoots, QoderTarget};
use std::collections::BTreeMap;

#[derive(Clone)]
struct Record {
    ts: i64,
    model: String,
    project: String,
    session: String,
    tokens: Option<Usage>,
    credits: Option<f64>,
    billable: Option<bool>,
}

fn nonnegative(value: Option<&Value>) -> Option<f64> {
    value?.as_f64().filter(|v| v.is_finite() && *v >= 0.0)
}

fn decode(value: &Value) -> Option<(String, Record)> {
    if value.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let message = value.get("message")?;
    let usage = message.get("usage")?;
    let ts = value.get("timestamp")?;
    let ts = ts.as_i64().or_else(|| {
        chrono::DateTime::parse_from_rfc3339(ts.as_str()?)
            .ok()
            .map(|t| t.timestamp_millis())
    })?;
    let model = non_empty_text(message.get("model")).unwrap_or_else(|| "未知模型".into());
    let session = non_empty_text(value.get("sessionId")).unwrap_or_else(|| "未知会话".into());
    // Do not return absolute project paths or message titles/content.
    let project = value
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(|cwd| cwd.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next())
        .filter(|name| !name.is_empty())
        .unwrap_or("未知项目")
        .to_string();
    let input = usage.get("input_tokens").and_then(Value::as_u64);
    let output = usage.get("output_tokens").and_then(Value::as_u64);
    let read = usage.get("cache_read_input_tokens").and_then(Value::as_u64);
    let write = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64);
    // Require all four fields: absent cache counts must not become fabricated 0s.
    let tokens =
        input
            .zip(output)
            .zip(read.zip(write))
            .and_then(|((input, output), (read, write))| {
                let usage = Usage {
                    input: input.saturating_add(read),
                    output,
                    read,
                    write,
                };
                (usage_total(usage) > 0).then_some(usage)
            });
    let credits = nonnegative(usage.get("credits"));
    if credits.is_none() && tokens.is_none() {
        return None;
    }
    let id = non_empty_text(usage.get("request_id"))
        .or_else(|| non_empty_text(message.get("id")))
        .or_else(|| non_empty_text(value.get("uuid")))
        .unwrap_or_else(|| {
            format!("{session}:{ts}:{model}:{input:?}:{output:?}:{read:?}:{write:?}:{credits:?}")
        });
    Some((
        id,
        Record {
            ts,
            model,
            project,
            session,
            tokens,
            credits,
            billable: usage.get("billable").and_then(Value::as_bool),
        },
    ))
}

#[derive(Default)]
struct CreditTotals {
    credits: f64,
    billable: f64,
    free: f64,
    unknown: f64,
    records: u64,
}
impl CreditTotals {
    fn add(&mut self, record: &Record) {
        let Some(value) = record.credits else {
            return;
        };
        self.credits += value;
        self.records += 1;
        match record.billable {
            Some(true) => self.billable += value,
            Some(false) => self.free += value,
            None => self.unknown += value,
        }
    }
    fn value(&self) -> Value {
        json!({"credits":self.credits,"billableCredits":self.billable,"nonBillableCredits":self.free,"unknownBillingCredits":self.unknown,"records":self.records})
    }
}

fn credit_groups(groups: BTreeMap<String, CreditTotals>) -> Vec<Value> {
    groups
        .into_iter()
        .map(|(key, total)| {
            let mut v = total.value();
            v["key"] = json!(key);
            v
        })
        .collect()
}

fn collect_files(root: &Path, paths: &mut Vec<PathBuf>, errors: &mut u64, depth: usize) {
    if depth > 12 {
        *errors += 1;
        return;
    }
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(_) => {
            *errors += 1;
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            *errors += 1;
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            *errors += 1;
            continue;
        };
        // Never follow symlinks/junctions out of the known session roots.
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            collect_files(&path, paths, errors, depth + 1);
        } else if kind.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            paths.push(path);
        }
    }
}

fn scan(root: &Path, source: &str, now: i64) -> (Value, Value) {
    let mut paths = Vec::new();
    let mut errors = 0;
    collect_files(root, &mut paths, &mut errors, 0);
    paths.sort();
    let mut records: HashMap<String, Record> = HashMap::new();
    for path in &paths {
        let Ok(file) = std::fs::File::open(path) else {
            errors += 1;
            continue;
        };
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else {
                errors += 1;
                continue;
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                errors += 1;
                continue;
            };
            if let Some((id, record)) = decode(&value) {
                if record.ts <= 0 || record.ts > now {
                    continue;
                }
                // Streaming updates, copied/forked histories and subagent copies
                // describe the same request. Prefer the latest final metadata.
                if records.get(&id).is_none_or(|old| record.ts >= old.ts) {
                    records.insert(id, record);
                }
            }
        }
    }
    let mut records: Vec<_> = records.into_values().collect();
    records.sort_by_key(|v| (v.ts, v.session.clone(), v.model.clone()));
    let mut tokens = SourceCollector {
        collect_requests: true,
        parse_errors: errors,
        ..Default::default()
    };
    let mut sessions: BTreeMap<(String, String), Totals> = BTreeMap::new();
    let mut total = CreditTotals::default();
    let mut daily: BTreeMap<String, CreditTotals> = BTreeMap::new();
    let mut models: BTreeMap<String, CreditTotals> = BTreeMap::new();
    let mut daily_by_model: BTreeMap<String, BTreeMap<String, CreditTotals>> = BTreeMap::new();
    let mut projects: BTreeMap<String, CreditTotals> = BTreeMap::new();
    let mut requests = Vec::new();
    let mut unknown_tokens = 0;
    for r in &records {
        let value = json!({"timestamp":r.ts,"model":r.model});
        if let Some(usage) = r.tokens {
            tokens.add(usage, &value, &r.project);
            tokens.add_account_usage(AttributedUsage {
                usage,
                timestamp: Some(r.ts),
                model: r.model.clone(),
                account_id: None,
            });
            sessions
                .entry((r.session.clone(), r.project.clone()))
                .or_default()
                .add(usage);
            tokens.push_request(RequestRow {
                timestamp: r.ts,
                model: r.model.clone(),
                project: r.project.clone(),
                session_id: r.session.clone(),
                title: None,
                usage,
                thinking: 0,
            });
        } else {
            unknown_tokens += 1;
        }
        total.add(r);
        if let Some(day) = date(&value) {
            daily.entry(day.clone()).or_default().add(r);
            daily_by_model
                .entry(r.model.clone())
                .or_default()
                .entry(day)
                .or_default()
                .add(r);
        }
        models.entry(r.model.clone()).or_default().add(r);
        projects.entry(r.project.clone()).or_default().add(r);
    }
    // Bound detail output independently from the complete aggregate totals.
    for r in records
        .iter()
        .rev()
        .filter(|r| r.credits.is_some())
        .take(REQUEST_WINDOW)
    {
        requests.push(json!({"timestamp":r.ts,"model":r.model,"project":r.project,"sessionId":r.session,"credits":r.credits,"billable":r.billable}));
    }
    for ((session, project), totals) in sessions {
        tokens.push_session(session, None, project, totals);
    }
    let mut token_value = tokens.into_value(source, paths.len());
    token_value["unavailableTokenRecords"] = json!(unknown_tokens);
    if let Some(rows) = token_value["requests"].as_array_mut() {
        for row in rows {
            row["thinking"] = Value::Null;
        }
    }
    let daily_by_model: BTreeMap<_, _> = daily_by_model
        .into_iter()
        .map(|(model, days)| (model, credit_groups(days)))
        .collect();
    let credit_value = json!({"source":source,"summary":total.value(),"daily":credit_groups(daily),"dailyByModel":daily_by_model,"models":credit_groups(models),"projects":credit_groups(projects),"requests":requests,"filesScanned":paths.len(),"parseErrors":errors,"coverageStartAt":records.first().map(|r| r.ts),"coverageEndAt":records.last().map(|r| r.ts)});
    (token_value, credit_value)
}

pub fn get_statistics() -> Value {
    statistics_at(&QoderRoots::real(), crate::modules::config::now_ms())
}

fn statistics_at(roots: &QoderRoots, now: i64) -> Value {
    let (cn_tokens, cn_credits) = scan(
        &roots.cli_dir_for(QoderTarget::Cli).join("projects"),
        "qoder",
        now,
    );
    let (ai_tokens, ai_credits) = scan(
        &roots.cli_dir_for(QoderTarget::GlobalCli).join("projects"),
        "qoder-ai",
        now,
    );
    json!({"generatedAt":now,"sources":[cn_tokens,ai_tokens],"credits":[cn_credits,ai_credits]})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(id: &str, tokens: u64, billable: Option<bool>) -> Value {
        json!({"type":"assistant","timestamp":"2026-11-01T12:00:00Z","sessionId":"session","cwd":"D:\\private\\project","message":{"id":id,"model":"test-model","content":"DO NOT RETURN","usage":{"input_tokens":tokens,"output_tokens":tokens,"cache_read_input_tokens":tokens,"cache_creation_input_tokens":tokens,"credits":2.5,"billable":billable}}})
    }
    #[test]
    fn placeholders_are_unavailable_and_cache_is_counted_once() {
        assert!(decode(&record("a", 0, Some(false)))
            .unwrap()
            .1
            .tokens
            .is_none());
        let r = decode(&record("a", 10, Some(true))).unwrap().1;
        assert_eq!(usage_total(r.tokens.unwrap()), 40);
        assert_eq!(r.project, "project");
        let mut missing = record("a", 10, None);
        missing["message"]["usage"]
            .as_object_mut()
            .unwrap()
            .remove("output_tokens");
        assert!(decode(&missing).unwrap().1.tokens.is_none());
    }
    #[test]
    fn dedupes_copies_and_separates_regions_and_billing() {
        let base = std::env::temp_dir().join(format!("qoder-stats-{}", uuid::Uuid::new_v4()));
        let roots = QoderRoots {
            home: base.clone(),
            app_data: base.clone(),
        };
        let root = roots.cli_dir().join("projects");
        std::fs::create_dir_all(root.join("subagents")).unwrap();
        let data = [
            record("a", 0, Some(false)),
            record("b", 10, Some(true)),
            record("c", 0, None),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
        std::fs::write(root.join("one.jsonl"), format!("{data}\nmalformed\n")).unwrap();
        std::fs::write(root.join("subagents/copy.jsonl"), data).unwrap();
        let result = statistics_at(&roots, 1794000000000);
        let cn = &result["credits"][0];
        assert_eq!(cn["summary"]["credits"], 7.5);
        assert_eq!(cn["summary"]["billableCredits"], 2.5);
        assert_eq!(cn["summary"]["nonBillableCredits"], 2.5);
        assert_eq!(cn["summary"]["unknownBillingCredits"], 2.5);
        assert_eq!(result["sources"][0]["summary"]["total"], 40);
        assert_eq!(result["sources"][0]["unavailableTokenRecords"], 2);
        assert_eq!(result["credits"][1]["summary"]["records"], 0);
        assert_eq!(cn["parseErrors"], 1);
        assert!(!result.to_string().contains("DO NOT RETURN"));
        assert!(!result.to_string().contains("private"));
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn detail_window_does_not_truncate_totals_and_final_request_updates_replace() {
        let base = std::env::temp_dir().join(format!("qoder-window-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let mut rows = (0..600)
            .map(|i| record(&format!("request-{i}"), 10, Some(true)))
            .collect::<Vec<_>>();
        let mut updated = rows[0].clone();
        updated["timestamp"] = json!("2026-11-01T12:01:00Z");
        updated["message"]["usage"]["credits"] = json!(5);
        rows.push(updated);
        std::fs::write(
            base.join("records.jsonl"),
            rows.iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let (tokens, credits) = scan(&base, "qoder", 1794000000000);
        assert_eq!(tokens["summary"]["records"], 600);
        assert_eq!(tokens["summary"]["total"], 24000);
        assert_eq!(credits["summary"]["credits"], 1502.5);
        assert_eq!(credits["requests"].as_array().unwrap().len(), 500);
        assert_eq!(credits["requests"][0]["credits"], 5.0);
        assert!(tokens["requests"][0]["thinking"].is_null());
        std::fs::remove_dir_all(base).unwrap();
    }
}
