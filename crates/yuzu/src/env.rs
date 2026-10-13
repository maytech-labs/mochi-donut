//! The process environment a command would run in, captured once so the
//! rest of yuzu can be tested without touching the real one.

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct Ctx {
    pub vars: HashMap<String, String>,
    pub cwd: PathBuf,
    pub home: PathBuf,
}

impl Ctx {
    pub fn from_process() -> Ctx {
        let vars: HashMap<String, String> = std::env::vars().collect();
        let home = vars.get("HOME").map(PathBuf::from).unwrap_or_default();
        Ctx {
            cwd: std::env::current_dir().unwrap_or_default(),
            vars,
            home,
        }
    }

    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    /// `~` and `~/x` expanded against `home`; anything relative resolved
    /// against `cwd`.
    pub fn path(&self, p: &str) -> PathBuf {
        let expanded = if p == "~" {
            self.home.clone()
        } else if let Some(rest) = p.strip_prefix("~/") {
            self.home.join(rest)
        } else {
            PathBuf::from(p)
        };
        if expanded.is_absolute() {
            expanded
        } else {
            self.cwd.join(expanded)
        }
    }
}
