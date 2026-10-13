//! `taro reviews`: open review threads as a checklist (docs/taro/design.md,
//! "taro reviews").

use serde::Serialize;
use serde_json::Value;

pub const QUERY: &str = "query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:100){nodes{isResolved isOutdated path line comments(last:1){nodes{author{login} body url}} first:comments(first:1){nodes{author{login}}}}}}}}";

#[derive(Debug, Clone, Serialize)]
pub struct Thread {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    /// Who opened it.
    pub reviewer: String,
    /// Who wrote the latest comment, and what it says.
    pub last_author: String,
    pub last_comment: String,
    pub url: String,
    /// The code the thread is on has changed since.
    pub outdated: bool,
    pub resolved: bool,
}

/// Every thread in a `QUERY` answer.
pub fn threads(answer: &Value) -> Vec<Thread> {
    answer
        .pointer("/data/repository/pullRequest/reviewThreads/nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|t| {
            let last = &t["comments"]["nodes"][0];
            Thread {
                path: t["path"].as_str().unwrap_or("").to_owned(),
                line: t["line"].as_u64(),
                reviewer: t["first"]["nodes"][0]["author"]["login"]
                    .as_str()
                    .unwrap_or("?")
                    .to_owned(),
                last_author: last["author"]["login"].as_str().unwrap_or("?").to_owned(),
                last_comment: last["body"].as_str().unwrap_or("").trim().to_owned(),
                url: last["url"].as_str().unwrap_or("").to_owned(),
                outdated: t["isOutdated"].as_bool().unwrap_or(false),
                resolved: t["isResolved"].as_bool().unwrap_or(false),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_threads() {
        let answer = json!({"data": {"repository": {"pullRequest": {"reviewThreads": {"nodes": [
            {"isResolved": false, "isOutdated": true, "path": "src/a.rs", "line": 12,
             "comments": {"nodes": [{"author": {"login": "me"}, "body": " Fixed? ", "url": "u"}]},
             "first": {"nodes": [{"author": {"login": "rev"}}]}},
            {"isResolved": true, "isOutdated": false, "path": "b.rs", "line": null,
             "comments": {"nodes": []}, "first": {"nodes": []}}
        ]}}}}});
        let t = threads(&answer);
        assert_eq!(t.len(), 2);
        assert_eq!(
            (
                t[0].reviewer.as_str(),
                t[0].last_author.as_str(),
                t[0].last_comment.as_str()
            ),
            ("rev", "me", "Fixed?")
        );
        assert!(t[0].outdated && !t[0].resolved);
        assert_eq!(t[1].line, None);
    }
}
