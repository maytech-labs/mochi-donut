//! Adding patterns to the environments file in place, keeping the user's
//! comments and layout (docs/decisions/0037-yuzu-env-add.md).

use std::path::Path;

use serde::Serialize;
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

use crate::config::{Class, Config, Kind};
use crate::policy::class_name;

/// What `add` changed.
#[derive(Debug, Serialize)]
pub struct Added {
    pub path: String,
    pub env: String,
    pub key: &'static str,
    /// Whether the env didn't exist and was made.
    pub created_env: bool,
    /// Patterns written.
    pub added: Vec<String>,
    /// Patterns the env already had, left alone.
    pub already: Vec<String>,
    /// Added names an earlier env still matches first, so the add changes
    /// nothing for them.
    pub shadowed: Vec<Shadowed>,
    #[serde(skip)]
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct Shadowed {
    pub pattern: String,
    pub env: String,
}

/// Adds `patterns` to `key` in the env named `env`, in the file text `text`.
/// Makes the env when it doesn't exist, which needs a `class`. The result
/// must load as a config, so this can't write a file yuzu would refuse.
pub fn add(
    text: &str,
    path: &Path,
    env: &str,
    kind: Kind,
    patterns: &[String],
    class: Option<Class>,
) -> Result<Added, String> {
    let mut doc: DocumentMut = text
        .parse()
        .map_err(|e| format!("{} isn't valid TOML: {e}", path.display()))?;
    let envs = match doc.get_mut("env") {
        None => {
            doc.insert("env", Item::ArrayOfTables(ArrayOfTables::new()));
            doc.get_mut("env").and_then(Item::as_array_of_tables_mut)
        }
        Some(item) => item.as_array_of_tables_mut(),
    }
    .ok_or_else(|| format!("{}: `env` should be [[env]] tables.", path.display()))?;

    let key = kind.key();
    let existing = envs
        .iter_mut()
        .find(|t| t.get("name").and_then(Item::as_str) == Some(env));
    let created_env = existing.is_none();
    let table = match existing {
        Some(t) => {
            let current = t.get("class").and_then(Item::as_str);
            if let Some(class) = class
                && current != Some(class_name(Some(class)))
            {
                return Err(format!(
                    "{env} is already class {}. Leave out --class to add to it, or pick another env name.",
                    current.unwrap_or("unset")
                ));
            }
            t
        }
        None => {
            let Some(class) = class else {
                return Err(format!(
                    "There's no env named {env}. Add --class mine (or work, production) to make it."
                ));
            };
            let mut t = Table::new();
            t.insert("name", value(env));
            t.insert("class", value(class_name(Some(class))));
            envs.push(t);
            let last = envs.len() - 1;
            envs.get_mut(last)
                .ok_or_else(|| "couldn't add the env".to_owned())?
        }
    };

    if !table.contains_key(key) {
        table.insert(key, value(Array::new()));
    }
    let array = table
        .get_mut(key)
        .and_then(Item::as_array_mut)
        .ok_or_else(|| format!("{env}'s {key} should be a list of strings."))?;
    let (mut added, mut already) = (Vec::new(), Vec::new());
    for p in patterns {
        if array.iter().any(|v| v.as_str() == Some(p)) || added.contains(p) {
            already.push(p.clone());
        } else {
            array.push(p.as_str());
            added.push(p.clone());
        }
    }

    let text = doc.to_string();
    let config = Config::parse(&text, path)?;
    let shadowed = added
        .iter()
        .filter_map(|p| {
            let first = config.classify(kind, p)?;
            (first.name != env).then(|| Shadowed {
                pattern: p.clone(),
                env: first.name.clone(),
            })
        })
        .collect();
    Ok(Added {
        shadowed,
        path: path.display().to_string(),
        env: env.to_owned(),
        key,
        created_env,
        added,
        already,
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"# Whose environments are whose.

[[env]]
name = "homelab"
class = "mine"
ssh_hosts = ["samwise", "merry"] # the pis

[[env]]
name = "work"
class = "work"
kube_contexts = ["*-nonprod-*"]
"#;

    fn run(text: &str, env: &str, kind: Kind, p: &[&str], class: Option<Class>) -> Added {
        let p: Vec<String> = p.iter().map(|s| (*s).to_owned()).collect();
        add(text, Path::new("envs.toml"), env, kind, &p, class).unwrap()
    }

    #[test]
    fn adds_to_an_existing_list_and_keeps_comments() {
        let a = run(FILE, "homelab", Kind::SshHost, &["balin"], None);
        assert_eq!(a.added, ["balin"]);
        assert!(!a.created_env);
        assert!(a.text.starts_with("# Whose environments are whose."));
        assert!(
            a.text
                .contains(r#"ssh_hosts = ["samwise", "merry", "balin"] # the pis"#),
            "{}",
            a.text
        );
        let c = Config::parse(&a.text, Path::new("x")).unwrap();
        assert_eq!(
            c.classify(Kind::SshHost, "balin").map(|e| e.name.as_str()),
            Some("homelab")
        );
    }

    #[test]
    fn makes_the_key_when_the_env_lacks_it() {
        let a = run(FILE, "work", Kind::SshHost, &["bastion"], None);
        assert!(a.text.contains(r#"ssh_hosts = ["bastion"]"#), "{}", a.text);
    }

    #[test]
    fn makes_the_env_when_given_a_class() {
        let a = run(
            FILE,
            "lab",
            Kind::GitRemote,
            &["github.com/me/*"],
            Some(Class::Mine),
        );
        assert!(a.created_env);
        let c = Config::parse(&a.text, Path::new("x")).unwrap();
        assert_eq!(c.envs.len(), 3);
        assert_eq!(c.envs[2].class, Class::Mine);
        let a = run("", "lab", Kind::SshHost, &["pi-1"], Some(Class::Mine));
        assert_eq!(
            Config::parse(&a.text, Path::new("x")).unwrap().envs.len(),
            1
        );
    }

    #[test]
    fn a_new_env_needs_a_class_and_an_old_one_keeps_its_own() {
        let p = vec!["x".to_owned()];
        let err = add(FILE, Path::new("f"), "lab", Kind::SshHost, &p, None).unwrap_err();
        assert!(err.contains("--class mine"), "{err}");
        let err = add(
            FILE,
            Path::new("f"),
            "work",
            Kind::SshHost,
            &p,
            Some(Class::Mine),
        )
        .unwrap_err();
        assert!(err.contains("already class work"), "{err}");
    }

    #[test]
    fn existing_patterns_are_reported_not_repeated() {
        let a = run(
            FILE,
            "homelab",
            Kind::SshHost,
            &["merry", "balin", "balin"],
            None,
        );
        assert_eq!(a.added, ["balin"]);
        assert_eq!(a.already, ["merry", "balin"]);
        assert_eq!(a.text.matches("balin").count(), 1);
    }

    #[test]
    fn says_when_an_earlier_env_still_matches() {
        let a = run(
            FILE,
            "lab",
            Kind::KubeContext,
            &["a-nonprod-1"],
            Some(Class::Mine),
        );
        assert_eq!(a.shadowed.len(), 1);
        assert_eq!(a.shadowed[0].env, "work");
        let a = run(FILE, "homelab", Kind::SshHost, &["balin"], None);
        assert!(a.shadowed.is_empty());
    }

    #[test]
    fn a_broken_file_is_refused() {
        let p = vec!["x".to_owned()];
        assert!(add("[[env", Path::new("f"), "a", Kind::SshHost, &p, None).is_err());
        assert!(add("env = 1", Path::new("f"), "a", Kind::SshHost, &p, None).is_err());
    }
}
