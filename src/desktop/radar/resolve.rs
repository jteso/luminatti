//! Resolve only paths evidenced by the compared snapshots, never global names.
use super::Snapshot;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub(super) fn json_config(source: &str) -> Option<Value> {
    let mut bytes = source.as_bytes().to_vec();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else if bytes.get(i..i + 2) == Some(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                bytes[i] = b' ';
                i += 1;
            }
        } else if bytes.get(i..i + 2) == Some(b"/*") {
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            while i < bytes.len() && bytes.get(i..i + 2) != Some(b"*/") {
                bytes[i] = b' ';
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return None;
            }
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
        } else {
            i += 1;
        }
    }
    i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
        } else {
            if bytes[i] == b','
                && bytes[i + 1..]
                    .iter()
                    .find(|b| !b.is_ascii_whitespace())
                    .is_some_and(|b| matches!(b, b'}' | b']'))
            {
                bytes[i] = b' ';
            }
            i += 1;
        }
    }
    serde_json::from_slice(&bytes).ok()
}
fn join(base: &str, path: &str) -> Option<String> {
    let joined = Path::new(base).join(path);
    let mut result = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                if !result.pop() {
                    return None;
                }
            }
            Component::Normal(name) => result.push(name),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(result.to_str()?.into())
}
fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}
fn contains(dir: &str, path: &str) -> bool {
    dir.is_empty() || path.strip_prefix(dir).is_some_and(|s| s.starts_with('/'))
}

#[derive(Clone, Default)]
struct Paths {
    base: Option<String>,
    aliases: BTreeMap<String, (String, Vec<String>)>,
}
fn config(snapshot: &Snapshot, path: &str, seen: &mut BTreeSet<String>) -> Paths {
    if !seen.insert(path.into()) {
        return Paths::default();
    }
    let Some(value) = snapshot.metadata.get(path) else {
        return Paths::default();
    };
    let dir = directory(path);
    let mut result = Paths::default();
    let parents = value
        .get("extends")
        .map(|v| v.as_array().cloned().unwrap_or_else(|| vec![v.clone()]))
        .unwrap_or_default();
    for parent in parents
        .iter()
        .filter_map(Value::as_str)
        .filter(|s| s.starts_with('.'))
    {
        if let Some(mut target) = join(dir, parent) {
            if !target.ends_with(".json") {
                target.push_str(".json");
            }
            let inherited = config(snapshot, &target, seen);
            if inherited.base.is_some() {
                result.base = inherited.base;
            }
            result.aliases.extend(inherited.aliases);
        }
    }
    if let Some(options) = value.get("compilerOptions") {
        if let Some(base) = options.get("baseUrl").and_then(Value::as_str) {
            result.base = join(dir, base);
        }
        if let Some(paths) = options.get("paths").and_then(Value::as_object) {
            result.aliases.clear();
            for (name, values) in paths {
                result.aliases.insert(
                    name.clone(),
                    (
                        result.base.clone().unwrap_or_else(|| dir.into()),
                        values
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect(),
                    ),
                );
            }
        }
    }
    result
}

pub(super) struct Resolver<'a> {
    snapshot: &'a Snapshot,
    packages: Vec<(String, &'a Value)>,
    configs: Vec<(String, Paths)>,
}
impl<'a> Resolver<'a> {
    pub fn new(snapshot: &'a Snapshot) -> Self {
        let mut packages: Vec<(String, &Value)> = Vec::new();
        let mut configs: Vec<(String, Paths)> = Vec::new();
        for (path, value) in &snapshot.metadata {
            if Path::new(path)
                .file_name()
                .is_some_and(|n| n == "package.json")
            {
                packages.push((directory(path).into(), value));
            }
            if Path::new(path)
                .file_name()
                .is_some_and(|n| n == "tsconfig.json" || n == "jsconfig.json")
            {
                configs.push((
                    directory(path).into(),
                    config(snapshot, path, &mut BTreeSet::new()),
                ));
            }
        }
        packages.sort_by_key(|(dir, _)| std::cmp::Reverse(dir.len()));
        configs.sort_by_key(|(dir, _)| std::cmp::Reverse(dir.len()));
        Self {
            snapshot,
            packages,
            configs,
        }
    }
    pub fn package(&self, path: &str) -> String {
        self.packages
            .iter()
            .find(|(dir, _)| contains(dir, path))
            .map(|(dir, v)| {
                v.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(if dir.is_empty() { "repository" } else { dir })
                    .into()
            })
            .unwrap_or_else(|| {
                let dir = directory(path);
                if dir.is_empty() {
                    "repository".into()
                } else {
                    dir.into()
                }
            })
    }
    fn file(&self, path: &str) -> Option<String> {
        if self.snapshot.modules.contains_key(path) {
            return Some(path.into());
        }
        let stem = [".js", ".jsx", ".mjs", ".cjs"]
            .iter()
            .find_map(|ext| path.strip_suffix(ext))
            .unwrap_or(path);
        for ext in ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "d.ts"] {
            for candidate in [format!("{stem}.{ext}"), format!("{path}/index.{ext}")] {
                if self.snapshot.modules.contains_key(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    }
    fn target(&self, dir: &str, value: &Value, star: Option<&str>) -> Option<String> {
        match value {
            Value::String(path) => self.file(&join(dir, &path.replace('*', star.unwrap_or("")))?),
            Value::Array(values) => values.iter().find_map(|v| self.target(dir, v, star)),
            Value::Object(values) => [
                "source",
                "development",
                "import",
                "require",
                "default",
                "types",
            ]
            .iter()
            .filter_map(|k| values.get(*k))
            .find_map(|v| self.target(dir, v, star)),
            _ => None,
        }
    }
    fn package_target(&self, dir: &str, value: &Value, sub: &str) -> Option<String> {
        if sub == "." {
            if let Some(target) = value.get("source").and_then(|v| self.target(dir, v, None)) {
                return Some(target);
            }
        }
        if let Some(exports) = value.get("exports") {
            if let Some(target) = exports.get(sub).and_then(|v| self.target(dir, v, None)) {
                return Some(target);
            }
            if sub == "." {
                if let Some(target) = self.target(dir, exports, None) {
                    return Some(target);
                }
            }
            if let Some(map) = exports.as_object() {
                for (pattern, value) in map {
                    if let Some((prefix, suffix)) = pattern.split_once('*') {
                        if let Some(star) = sub
                            .strip_prefix(prefix)
                            .and_then(|s| s.strip_suffix(suffix))
                        {
                            if let Some(target) = self.target(dir, value, Some(star)) {
                                return Some(target);
                            }
                        }
                    }
                }
            }
            return None;
        }
        if sub == "." {
            for field in ["module", "main", "types"] {
                if let Some(target) = value.get(field).and_then(|v| self.target(dir, v, None)) {
                    return Some(target);
                }
            }
            self.file(&join(dir, "index")?)
        } else {
            self.file(&join(dir, sub)?)
        }
    }
    pub fn is_entry(&self, path: &str) -> bool {
        self.packages.iter().any(|(dir, value)| {
            ["source", "main", "module"]
                .iter()
                .filter_map(|key| value.get(*key))
                .any(|v| self.target(dir, v, None).as_deref() == Some(path))
                || self.package_target(dir, value, ".").as_deref() == Some(path)
                || value.get("bin").is_some_and(|bin| {
                    self.target(dir, bin, None).as_deref() == Some(path)
                        || bin.as_object().is_some_and(|map| {
                            map.values()
                                .any(|v| self.target(dir, v, None).as_deref() == Some(path))
                        })
                })
        })
    }
    pub fn resolve(&self, importer: &str, source: &str) -> Option<String> {
        if source.starts_with('.') {
            return self.file(&join(directory(importer), source)?);
        }
        if let Some((_, paths)) = self.configs.iter().find(|(dir, _)| contains(dir, importer)) {
            // Exact matches win; otherwise use the longest wildcard prefix.
            let mut aliases = paths.aliases.iter().collect::<Vec<_>>();
            aliases.sort_by_key(|(key, _)| {
                (
                    key.contains('*'),
                    std::cmp::Reverse(key.split('*').next().unwrap_or("").len()),
                )
            });
            for (pattern, (base, targets)) in aliases {
                let matched = if let Some((prefix, suffix)) = pattern.split_once('*') {
                    source
                        .strip_prefix(prefix)
                        .and_then(|s| s.strip_suffix(suffix))
                } else {
                    (pattern == source).then_some("")
                };
                if let Some(star) = matched {
                    for target in targets {
                        if let Some(file) =
                            join(base, &target.replace('*', star)).and_then(|p| self.file(&p))
                        {
                            return Some(file);
                        }
                    }
                    break;
                }
            }
            if let Some(base) = &paths.base {
                if let Some(file) = join(base, source).and_then(|p| self.file(&p)) {
                    return Some(file);
                }
            }
        }
        for (dir, value) in &self.packages {
            let Some(name) = value.get("name").and_then(Value::as_str) else {
                continue;
            };
            if source == name {
                return self.package_target(dir, value, ".");
            }
            if let Some(sub) = source.strip_prefix(name).and_then(|s| s.strip_prefix('/')) {
                return self.package_target(dir, value, &format!("./{sub}"));
            }
        }
        None
    }
    pub fn edges(&self) -> (BTreeSet<(String, String)>, BTreeSet<(String, String)>) {
        let mut edges = BTreeSet::new();
        let mut unknown = BTreeSet::new();
        for (path, module) in &self.snapshot.modules {
            for source in &module.dependencies {
                if let Some(target) = self.resolve(path, source) {
                    if target != *path {
                        edges.insert((path.clone(), target));
                    }
                } else {
                    unknown.insert((path.clone(), source.clone()));
                }
            }
            for index in 0..module.unresolved_dynamic {
                unknown.insert((path.clone(), format!("<dynamic:{index}>")));
            }
        }
        (edges, unknown)
    }
}
