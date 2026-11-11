//! Read canonical MiniMax v2 transcripts; never expose prompts, responses or
//! credentials. The shared store has no trustworthy region/account attribution.
use super::*;
use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, Read};

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct Totals {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    total: u64,
    requests: u64,
}
impl Totals {
    fn add(&mut self, other: &Self) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
        self.total = self.total.saturating_add(other.total);
        self.requests = self.requests.saturating_add(other.requests);
    }
}
struct Usage {
    id: String,
    at: i64,
    model: String,
    tokens: Totals,
}
fn decode(value: &Value) -> Option<Usage> {
    let message = value.get("message")?;
    if message["role"] != "assistant" {
        return None;
    }
    let u = &message["usage"];
    let input = u["input"].as_u64()?;
    let output = u["output"].as_u64()?;
    let cache_read = u["cacheRead"].as_u64()?;
    let cache_write = u["cacheWrite"].as_u64()?;
    let total = input
        .checked_add(output)?
        .checked_add(cache_read)?
        .checked_add(cache_write)?;
    if u["totalTokens"].as_u64().is_some_and(|n| n != total) {
        return None;
    }
    let at = message["timestamp"].as_i64().or_else(|| {
        chrono::DateTime::parse_from_rfc3339(message["timestamp"].as_str()?)
            .ok()
            .map(|d| d.timestamp_millis())
    })?;
    let id = message["responseId"]
        .as_str()
        .filter(|v| !v.is_empty())
        .or_else(|| value["message_id"].as_str())?
        .to_string();
    let model = message["model"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 200)
        .unwrap_or("未知模型")
        .to_string();
    Some(Usage {
        id,
        at,
        model,
        tokens: Totals {
            input,
            output,
            cache_read,
            cache_write,
            total,
            requests: 1,
        },
    })
}
fn files(
    root: &Path,
    depth: usize,
    result: &mut Vec<PathBuf>,
    skipped: &mut usize,
) -> Result<(), String> {
    if depth > 6 || result.len() >= 20_000 {
        *skipped += 1;
        return Ok(());
    }
    let entries = match std::fs::read_dir(root) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("无法读取 MiniMax 会话目录".into()),
    };
    for entry in entries {
        let Ok(entry) = entry else {
            *skipped += 1;
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            *skipped += 1;
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            if files(&entry.path(), depth + 1, result, skipped).is_err() {
                *skipped += 1;
            }
        } else if kind.is_file() && entry.file_name() == "messages.jsonl" {
            result.push(entry.path());
        }
    }
    Ok(())
}
pub(super) fn statistics(root: &Path) -> Result<Value, String> {
    let mut paths = Vec::new();
    let mut skipped = 0;
    files(root, 0, &mut paths, &mut skipped)?;
    paths.sort();
    let mut seen = HashSet::new();
    let mut totals = Totals::default();
    let mut daily: BTreeMap<(String, String), Totals> = BTreeMap::new();
    let mut models: BTreeMap<String, Totals> = BTreeMap::new();
    let mut sessions = 0;
    let mut scanned_bytes = 0u64;
    for path in &paths {
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let size = file.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
        if size > 64 * 1024 * 1024 || scanned_bytes.saturating_add(size) > 512 * 1024 * 1024 {
            skipped += 1;
            continue;
        }
        scanned_bytes += size;
        let mut reader = BufReader::new(file);
        let mut used = false;
        loop {
            let mut line = Vec::new();
            let bytes = (&mut reader)
                .take(2 * 1024 * 1024 + 1)
                .read_until(b'\n', &mut line)
                .map_err(|_| "无法读取 MiniMax 用量记录")?;
            if bytes == 0 {
                break;
            }
            if bytes > 2 * 1024 * 1024 {
                skipped += 1;
                break;
            }
            let value: Value = match serde_json::from_slice(&line) {
                Ok(v) => v,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let Some(usage) = decode(&value) else {
                if value["message"]["role"] == "assistant"
                    && value["message"].get("usage").is_some()
                {
                    skipped += 1;
                }
                continue;
            };
            let Some(at) = chrono::DateTime::from_timestamp_millis(usage.at) else {
                skipped += 1;
                continue;
            };
            if !seen.insert(usage.id) {
                continue;
            }
            let day = at
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string();
            totals.add(&usage.tokens);
            daily
                .entry((day, usage.model.clone()))
                .or_default()
                .add(&usage.tokens);
            models.entry(usage.model).or_default().add(&usage.tokens);
            used = true;
        }
        if used {
            sessions += 1;
        }
    }
    Ok(
        json!({"generatedAt":chrono::Utc::now().timestamp(),"available":totals.requests>0,"accountAttributionAvailable":false,"regionAttributionAvailable":false,
        "source":"minimax-v2-sessions","timezone":iana_time_zone::get_timezone().ok(),"totals":totals,"sessionCount":sessions,"fileCount":paths.len(),"skippedRecords":skipped,"complete":skipped==0,
        "daily":daily.into_iter().map(|((date,model),tokens)|json!({"date":date,"model":model,"tokens":tokens})).collect::<Vec<_>>(),
        "models":models.into_iter().map(|(model,tokens)|json!({"model":model,"tokens":tokens})).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_records_deduplicate_without_leaking_content() {
        let root = std::env::temp_dir().join(format!("minimax-usage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let m = json!({"message_id":"m1","message":{"role":"assistant","timestamp":1735689600000i64,"model":"MiniMax","content":"private text","usage":{"input":10,"output":2,"cacheRead":3,"cacheWrite":4,"totalTokens":19}}});
        std::fs::write(root.join("messages.jsonl"), format!("{m}\n{m}\n{{partial")).unwrap();
        let result = statistics(&root).unwrap();
        assert_eq!(result["totals"]["total"], 19);
        assert_eq!(result["totals"]["requests"], 1);
        assert_eq!(result["complete"], false);
        assert!(!result.to_string().contains("private text"));
        assert_eq!(result["accountAttributionAvailable"], false);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn incomplete_usage_is_unknown_and_empty_store_is_unavailable() {
        assert!(decode(&json!({"message_id":"a","message":{"role":"assistant","timestamp":1,"usage":{"input":1,"output":2}}})).is_none());
        let root = std::env::temp_dir().join(format!("absent-minimax-{}", uuid::Uuid::new_v4()));
        assert_eq!(statistics(&root).unwrap()["available"], false);
    }
}
