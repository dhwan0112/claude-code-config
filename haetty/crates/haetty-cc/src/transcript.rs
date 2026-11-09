//! Token use of Claude Code sessions, read from their transcripts (`~/.claude/projects/**/*.jsonl`).
//!
//! An API response is written as one or more `assistant` records (one per content block), each repeating the
//! response's `usage`, so a response is counted once, by `message.id`. Its usage is attributed to the tool call whose
//! result came right before it; `NO_TOOL` when a prompt or nothing came before. What is kept of the content is sizes,
//! counts, and the identifiers the report prints: a command, a path or a pattern, cut short. Nothing else.

use crate::json::Json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::BufRead;

/// The attribution key of responses that follow no tool result.
pub const NO_TOOL: &str = "(no tool result)";

/// Token counts of responses: the input side with its two cache counts, and the output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

impl Tokens {
    pub fn add(&mut self, o: &Tokens) {
        self.input = self.input.saturating_add(o.input);
        self.output = self.output.saturating_add(o.output);
        self.cache_write = self.cache_write.saturating_add(o.cache_write);
        self.cache_read = self.cache_read.saturating_add(o.cache_read);
    }

    /// Everything counted: input, output, cache write and cache read.
    pub fn sum(&self) -> u64 {
        self.input.saturating_add(self.output).saturating_add(self.cache_write).saturating_add(self.cache_read)
    }

    /// The share of the input side served from the cache: cache read / (input + cache write + cache read).
    pub fn hit_ratio(&self) -> Option<f64> {
        let side = self.input.saturating_add(self.cache_write).saturating_add(self.cache_read);
        if side == 0 {
            None
        } else {
            Some(self.cache_read as f64 / side as f64)
        }
    }
}

/// The responses attributed to one tool (or to `NO_TOOL`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ByTool {
    pub responses: u64,
    pub tokens: Tokens,
}

/// One tool result: the tool, the call's identifier (command, path or pattern) and the result's size in bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultSize {
    pub tool: String,
    pub target: String,
    pub bytes: u64,
}

/// What one transcript (or several, merged) says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub name: String,
    pub records: u64,
    pub bad_lines: u64,
    pub responses: u64,
    pub tool_calls: u64,
    pub totals: Tokens,
    pub by_tool: BTreeMap<String, ByTool>,
    pub results: Vec<ResultSize>,
}

/// The longest identifier printed for a call.
const TARGET_CHARS: usize = 120;

/// The identifier of a call that the report prints: the command, the file path or the pattern (or a subagent's
/// description). Control characters become spaces; long values are cut with an ellipsis. Other tools: empty.
fn target(name: &str, input: &Json) -> String {
    let field = |k: &str| input.get(k).and_then(Json::str).unwrap_or("");
    let raw = match name {
        "Bash" => field("command"),
        "Read" | "Edit" | "Write" | "NotebookEdit" => field("file_path"),
        "Grep" | "Glob" => field("pattern"),
        "Agent" | "Task" => field("description"),
        "WebFetch" => field("url"),
        "WebSearch" => field("query"),
        _ => "",
    };
    let flat: String = raw.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if flat.chars().count() > TARGET_CHARS {
        let cut: String = flat.chars().take(TARGET_CHARS).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

/// The size of a tool result in bytes: a string's length, or the summed length of an array's text parts.
fn result_bytes(content: Option<&Json>) -> u64 {
    match content {
        Some(Json::Str(s)) => s.len() as u64,
        Some(Json::Arr(parts)) => {
            parts.iter().filter_map(|p| p.get("text").and_then(Json::str)).map(|t| t.len() as u64).sum()
        }
        _ => 0,
    }
}

#[derive(Default)]
struct Reader {
    report: Report,
    /// tool_use id -> (tool, identifier), from the assistant records seen so far
    calls: HashMap<String, (String, String)>,
    /// message ids already counted
    seen: HashSet<String>,
    /// the tool of the latest result, until the next response takes it
    last_result: Option<String>,
}

impl Reader {
    fn push(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        self.report.records += 1;
        let Ok(v) = Json::parse(line) else {
            self.report.bad_lines += 1;
            return;
        };
        match v.get("type").and_then(Json::str) {
            Some("assistant") => self.assistant(&v),
            Some("user") => self.user(&v),
            _ => {}
        }
    }

    fn assistant(&mut self, v: &Json) {
        let Some(msg) = v.get("message") else { return };
        for c in msg.get("content").and_then(Json::arr).unwrap_or(&[]) {
            if c.get("type").and_then(Json::str) != Some("tool_use") {
                continue;
            }
            self.report.tool_calls += 1;
            let id = c.get("id").and_then(Json::str).unwrap_or_default().to_string();
            let name = c.get("name").and_then(Json::str).unwrap_or("?").to_string();
            let t = c.get("input").map(|i| target(&name, i)).unwrap_or_default();
            self.calls.insert(id, (name, t));
        }
        let Some(id) = msg.get("id").and_then(Json::str) else { return };
        if !self.seen.insert(id.to_string()) {
            return; // another content block of a response already counted
        }
        let Some(u) = msg.get("usage") else { return };
        let n = |k: &str| u.get(k).and_then(Json::num).map(|x| x as u64).unwrap_or(0);
        let t = Tokens {
            input: n("input_tokens"),
            output: n("output_tokens"),
            cache_write: n("cache_creation_input_tokens"),
            cache_read: n("cache_read_input_tokens"),
        };
        self.report.responses += 1;
        self.report.totals.add(&t);
        let key = self.last_result.take().unwrap_or_else(|| NO_TOOL.to_string());
        let e = self.report.by_tool.entry(key).or_default();
        e.responses += 1;
        e.tokens.add(&t);
    }

    fn user(&mut self, v: &Json) {
        match v.get("message").and_then(|m| m.get("content")) {
            Some(Json::Arr(items)) => {
                let mut any = false;
                for item in items {
                    if item.get("type").and_then(Json::str) != Some("tool_result") {
                        continue;
                    }
                    any = true;
                    let id = item.get("tool_use_id").and_then(Json::str).unwrap_or_default();
                    let (name, t) = self.calls.get(id).cloned().unwrap_or_else(|| ("?".to_string(), String::new()));
                    let bytes = result_bytes(item.get("content"));
                    self.report.results.push(ResultSize { tool: name.clone(), target: t, bytes });
                    self.last_result = Some(name);
                }
                if !any {
                    self.last_result = None;
                }
            }
            // a prompt typed by the user: the next response follows no tool result
            _ => self.last_result = None,
        }
    }

    fn finish(mut self, name: &str) -> Report {
        self.report.name = name.to_string();
        self.report
    }
}

/// One transcript, read line by line; `name` is what the report calls it (usually the file name).
pub fn analyze_reader(name: &str, reader: impl BufRead) -> std::io::Result<Report> {
    let mut rd = Reader::default();
    for line in reader.lines() {
        rd.push(&line?);
    }
    Ok(rd.finish(name))
}

/// The same for text already in memory.
pub fn analyze_str(name: &str, text: &str) -> Report {
    let mut rd = Reader::default();
    for line in text.lines() {
        rd.push(line);
    }
    rd.finish(name)
}

/// Several reports as one: totals and tools added up, the results of all of them.
pub fn merge(name: &str, reports: &[Report]) -> Report {
    let mut m = Report { name: name.to_string(), ..Report::default() };
    for r in reports {
        m.records += r.records;
        m.bad_lines += r.bad_lines;
        m.responses += r.responses;
        m.tool_calls += r.tool_calls;
        m.totals.add(&r.totals);
        for (k, v) in &r.by_tool {
            let e = m.by_tool.entry(k.clone()).or_default();
            e.responses += v.responses;
            e.tokens.add(&v.tokens);
        }
        m.results.extend(r.results.iter().cloned());
    }
    m
}

/// A number with thousands separators (`1,234,567`).
pub fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// The report as text: the totals, the tools (most tokens first), and the `top` largest tool results.
pub fn render(r: &Report, top: usize) -> String {
    let mut o = String::new();
    let bad = if r.bad_lines > 0 { format!(", {} unreadable lines", r.bad_lines) } else { String::new() };
    let _ = writeln!(
        o,
        "{}: {} records, {} API responses, {} tool calls{bad}",
        r.name,
        grouped(r.records),
        grouped(r.responses),
        grouped(r.tool_calls)
    );
    let hit = r.totals.hit_ratio().map(|h| format!("{:.1} %", h * 100.0)).unwrap_or_else(|| "-".into());
    let t = &r.totals;
    let _ = writeln!(
        o,
        "total: input {}, output {}, cache write {}, cache read {}; sum {}; cache hit {}",
        grouped(t.input),
        grouped(t.output),
        grouped(t.cache_write),
        grouped(t.cache_read),
        grouped(t.sum()),
        hit
    );
    let _ = writeln!(o, "by tool (the response that followed the tool's result):");
    let _ = writeln!(
        o,
        "  {:<22} {:>10} {:>12} {:>12} {:>12} {:>14}",
        "tool", "responses", "input", "output", "cache write", "cache read"
    );
    let mut rows: Vec<(&String, &ByTool)> = r.by_tool.iter().collect();
    rows.sort_by(|a, b| b.1.tokens.sum().cmp(&a.1.tokens.sum()).then(a.0.cmp(b.0)));
    for (name, b) in rows {
        let _ = writeln!(
            o,
            "  {:<22} {:>10} {:>12} {:>12} {:>12} {:>14}",
            name,
            grouped(b.responses),
            grouped(b.tokens.input),
            grouped(b.tokens.output),
            grouped(b.tokens.cache_write),
            grouped(b.tokens.cache_read)
        );
    }
    let mut res: Vec<&ResultSize> = r.results.iter().collect();
    res.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.tool.cmp(&b.tool)));
    let _ = writeln!(o, "largest tool results ({} of {}):", top.min(res.len()), res.len());
    let _ = writeln!(o, "  {:>12}  {:<8} target", "bytes", "tool");
    for x in res.iter().take(top) {
        let _ = writeln!(o, "  {:>12}  {:<8} {}", grouped(x.bytes), x.tool, x.target);
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = concat!(
        r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}],"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000}}}"#,
        "\n",
        r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"x"}],"usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000}}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"0123456789"}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"text","text":"y"}],"usage":{"input_tokens":3,"output_tokens":4,"cache_creation_input_tokens":0,"cache_read_input_tokens":2000}}}"#,
        "\n",
        "{\n",
    );

    #[test]
    fn counts_each_response_once_and_attributes_it_to_the_preceding_tool_result() {
        let r = analyze_str("s", SAMPLE);
        assert_eq!(r.records, 6);
        assert_eq!(r.bad_lines, 1);
        assert_eq!(r.responses, 2);
        assert_eq!(r.tool_calls, 1);
        assert_eq!(r.totals, Tokens { input: 13, output: 9, cache_write: 100, cache_read: 3000 });
        // m1 came after the prompt: no tool result; m2 came after the Bash result
        let none = r.by_tool[NO_TOOL];
        assert_eq!((none.responses, none.tokens.input, none.tokens.cache_read), (1, 10, 1000));
        let bash = r.by_tool["Bash"];
        assert_eq!((bash.responses, bash.tokens.input, bash.tokens.cache_read), (1, 3, 2000));
        assert_eq!(r.results, vec![ResultSize { tool: "Bash".into(), target: "cargo test".into(), bytes: 10 }]);
    }

    #[test]
    fn hit_ratio_is_cache_read_over_the_input_side() {
        let t = Tokens { input: 13, output: 9, cache_write: 100, cache_read: 3000 };
        let h = t.hit_ratio().unwrap();
        assert!((h - 3000.0 / 3113.0).abs() < 1e-12);
        assert_eq!(Tokens::default().hit_ratio(), None);
    }

    #[test]
    fn identifiers_are_flat_and_cut() {
        let long = "x".repeat(200);
        let input = Json::parse(&format!(r#"{{"command":"a\tb{long}"}}"#)).unwrap();
        let t = target("Bash", &input);
        assert_eq!(t.chars().count(), TARGET_CHARS + 1);
        assert!(t.starts_with("a b"));
        assert!(t.ends_with('…'));
        assert_eq!(
            target("Edit", &Json::parse(r#"{"file_path":"src/a.rs","old_string":"secret"}"#).unwrap()),
            "src/a.rs"
        );
        assert_eq!(
            target("WebFetch", &Json::parse(r#"{"url":"https://example.org/a","prompt":"p"}"#).unwrap()),
            "https://example.org/a"
        );
    }

    #[test]
    fn groups_thousands_and_merges() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(1234567), "1,234,567");
        let a = analyze_str("a", SAMPLE);
        let m = merge("both", &[a.clone(), a]);
        assert_eq!(m.responses, 4);
        assert_eq!(m.totals.input, 26);
        assert_eq!(m.by_tool["Bash"].responses, 2);
        assert_eq!(m.results.len(), 2);
    }

    #[test]
    fn render_lists_the_tools_and_the_largest_results() {
        let text = render(&analyze_str("s", SAMPLE), 5);
        assert!(text.contains("2 API responses, 1 tool calls, 1 unreadable lines"), "{text}");
        assert!(text.contains("cache hit 96.4 %"), "{text}");
        assert!(text.contains("Bash"), "{text}");
        assert!(text.contains("cargo test"), "{text}");
    }
}
