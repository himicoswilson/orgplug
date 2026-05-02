#!/usr/bin/env rust-script
//! Single-file Rust CLI for org-plugins.
//! Commands: doctor | update | build | sync [--platform auto|macos|windows] [--dest PATH]

use std::collections::{BTreeSet, HashMap, HashSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Default)]
struct DenyList {
    deny: HashSet<String>,
}

#[derive(Debug, Clone, Default)]
struct RepoRule {
    sync: Option<bool>,
    plugins: DenyList,
    skills: DenyList,
}

#[derive(Debug, Clone, Default)]
struct Config {
    repos: HashMap<String, RepoRule>,
    plugins_sync: HashMap<String, bool>,
}


fn main() {
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let repo_root = resolve_repo_root()?;

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    match args[1].as_str() {
        "doctor" => cmd_doctor(&repo_root),
        "update" => cmd_update(&repo_root),
        "build" => cmd_build(&repo_root),
        "sync" => cmd_sync(&repo_root, &args[2..]),
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(())
        }
        other => Err(format!("unknown command: {other}")),
    }
}

fn print_usage() {
    println!("Usage:");
    println!("  orgplug doctor");
    println!("  orgplug update");
    println!("  orgplug build");
    println!("  orgplug sync [--platform auto|macos|windows] [--dest PATH]");
}

fn cmd_doctor(repo_root: &Path) -> Result<(), String> {
    ensure_tool("git")?;
    ensure_tool("jq")?;

    let config_path = resolve_config_path(repo_root)?;
    let gitmodules = repo_root.join(".gitmodules");

    if !gitmodules.exists() {
        return Err("missing .gitmodules".to_string());
    }

    let repos = parse_gitmodules_paths(&gitmodules)?;
    let _cfg = parse_config(&config_path)?;

    let plugins_dir = repo_root.join("plugins");
    for rel in &repos {
        let p = repo_root.join(rel);
        if !p.exists() {
            return Err(format!("submodule path not found: {}", rel));
        }
        if rel.ends_with("knowledge-work-plugins") {
            validate_knowledge_work_structure(&p)?;
        }
        if rel.ends_with("anthropics-skills") {
            validate_anthropic_skills_structure(&p)?;
        }
    }

    if !plugins_dir.exists() {
        return Err("missing plugins directory".to_string());
    }

    println!("OK: doctor checks passed");
    Ok(())
}

fn cmd_update(repo_root: &Path) -> Result<(), String> {
    ensure_tool("git")?;

    let tracked_paths = parse_gitmodules_paths(&repo_root.join(".gitmodules"))?;
    warn_stale_submodule_gitlinks(repo_root, &tracked_paths)?;

    run_cmd(repo_root, "git", &["submodule", "sync", "--recursive"])?;

    for rel in tracked_paths {
        println!("==> {}", rel);
        run_cmd(repo_root, "git", &["submodule", "update", "--init", "--recursive", "--", &rel])?;

        let sm_path = repo_root.join(&rel);
        run_cmd(&sm_path, "git", &["fetch", "--prune", "--tags", "--force", "origin"])?;

        let branch = run_capture(&sm_path, "git", &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
        if !branch.is_empty() && branch != "HEAD" {
            let _ = run_cmd(&sm_path, "git", &["merge", "--ff-only", &format!("origin/{}", branch)]);
            continue;
        }

        let headref = run_capture(&sm_path, "git", &["symbolic-ref", "-q", "refs/remotes/origin/HEAD"]).unwrap_or_default();
        if !headref.is_empty() {
            let target = headref.trim_start_matches("refs/remotes/origin/");
            let _ = run_cmd(&sm_path, "git", &["checkout", "-q", target]);
            let _ = run_cmd(&sm_path, "git", &["merge", "--ff-only", &headref]);
        }
    }

    run_cmd(repo_root, "git", &["status", "--short"])?;
    Ok(())
}

fn cmd_build(repo_root: &Path) -> Result<(), String> {
    ensure_tool("git")?;
    ensure_tool("jq")?;

    let config = parse_config(&resolve_config_path(repo_root)?)?;
    let submodule_paths = parse_gitmodules_paths(&repo_root.join(".gitmodules"))?;

    let dist_root = repo_root.join("dist");
    let dist_plugins = dist_root.join("org-plugins");

    if dist_root.exists() {
        let _ = run_cmd(repo_root, "rm", &["-rf", dist_root.to_string_lossy().as_ref()]);
    }
    fs::create_dir_all(&dist_plugins).map_err(|e| format!("failed to create dist: {e}"))?;

    let mut used_names = HashSet::new();
    let mut managed_names = BTreeSet::new();

    for rel in submodule_paths {
        if repo_sync_decision(&config, &rel) == Decision::Deny {
            continue;
        }

        let sm_path = repo_root.join(&rel);
        if !sm_path.exists() {
            continue;
        }

        if rel.ends_with("knowledge-work-plugins") {
            build_knowledge_work_repo(
                repo_root,
                &config,
                &sm_path,
                &rel,
                &dist_plugins,
                &mut used_names,
                &mut managed_names,
            )?;
            continue;
        }

        if rel.ends_with("anthropics-skills") {
            build_anthropic_skills_repo(
                repo_root,
                &config,
                &sm_path,
                &rel,
                &dist_plugins,
                &mut used_names,
                &mut managed_names,
            )?;
            continue;
        }
    }

    remove_junk(&dist_root)?;

    println!("Built: {}", dist_plugins.display());
    println!("Built plugins: {}", managed_names.len());

    Ok(())
}

fn cmd_sync(repo_root: &Path, args: &[String]) -> Result<(), String> {
    let mut platform = "auto".to_string();
    let mut dest = String::new();

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--platform" => {
                let v = args.get(i + 1).ok_or("--platform requires value")?;
                platform = v.to_string();
                i += 2;
            }
            "--dest" => {
                let v = args.get(i + 1).ok_or("--dest requires value")?;
                dest = v.to_string();
                i += 2;
            }
            other => return Err(format!("unknown sync arg: {other}")),
        }
    }

    cmd_build(repo_root)?;

    let detected = if platform == "auto" {
        if cfg!(target_os = "macos") {
            "macos".to_string()
        } else if cfg!(target_os = "windows") {
            "windows".to_string()
        } else {
            "macos".to_string()
        }
    } else {
        platform
    };

    if dest.is_empty() {
        dest = if detected == "windows" {
            "C:\\Program Files\\Claude\\org-plugins".to_string()
        } else {
            "/Library/Application Support/Claude/org-plugins".to_string()
        };
    }

    if detected == "windows" && !cfg!(target_os = "windows") {
        eprintln!("This environment is not Windows; generated dist only.");
        eprintln!("Copy dist/org-plugins to: {}", dest);
        return Ok(());
    }

    let dest_path = PathBuf::from(dest);
    fs::create_dir_all(&dest_path).map_err(|e| format!("failed to create dest: {e}"))?;

    let dist_plugins = repo_root.join("dist/org-plugins");

    let mut count = 0usize;
    for entry in fs::read_dir(&dist_plugins).map_err(|e| format!("failed to read dist plugins: {e}"))? {
        let entry = entry.map_err(|e| format!("dir entry failed: {e}"))?;
        let src = entry.path();
        if !src.is_dir() {
            continue;
        }
        let name = src
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or("invalid dist plugin directory name")?;
        let safe_name = validate_name(name)?;
        let dst = dest_path.join(&safe_name);
        if dst.exists() {
            fs::remove_dir_all(&dst).map_err(|e| format!("failed to remove {}: {e}", dst.display()))?;
        }
        copy_dir_filtered(&src, &dst)?;
        count += 1;
    }

    println!("Synced {} plugins to: {}", count, dest_path.display());
    Ok(())
}

fn resolve_repo_root() -> Result<PathBuf, String> {
    let cwd = env::current_dir().map_err(|e| format!("failed to get current dir: {e}"))?;
    if cwd.join(".gitmodules").exists() && cwd.join("plugins").exists() {
        return Ok(cwd);
    }

    if let Some(home) = env::var_os("HOME") {
        let workdir = PathBuf::from(home).join(".orgplug").join("workdir").join("orgplug");
        if workdir.join(".gitmodules").exists() && workdir.join("plugins").exists() {
            return Ok(workdir);
        }
    }

    Err("repository workdir not found. Run from repo root or initialize ~/.orgplug/workdir/orgplug".to_string())
}

fn resolve_config_path(repo_root: &Path) -> Result<PathBuf, String> {
    if let Ok(p) = env::var("ORG_PLUGINS_CLI_CONFIG") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!("config not found: {}", path.display()));
    }

    if let Some(home) = env::var_os("HOME") {
        let path = PathBuf::from(home).join(".orgplug").join("config.yaml");
        if path.exists() {
            return Ok(path);
        }
    }

    let fallback = repo_root.join("config/config.yaml");
    if fallback.exists() {
        return Ok(fallback);
    }

    Err("no config found. Expected ~/.org_plugins_cli/config.yaml or config/config.yaml".to_string())
}

fn parse_gitmodules_paths(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("failed to read .gitmodules: {e}"))?;
    let mut paths = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("path =") {
            paths.push(rest.trim().to_string());
        }
    }
    if paths.is_empty() {
        return Err("no submodule paths in .gitmodules".to_string());
    }
    Ok(paths)
}

fn warn_stale_submodule_gitlinks(repo_root: &Path, tracked_paths: &[String]) -> Result<(), String> {
    let out = run_capture(repo_root, "git", &["ls-files", "--stage"])?;
    let tracked: HashSet<String> = tracked_paths.iter().cloned().collect();

    let mut stale: Vec<String> = Vec::new();
    for line in out.lines() {
        // format: <mode> <sha> <stage>\t<path>
        if !line.starts_with("160000 ") {
            continue;
        }
        if let Some((_, p)) = line.split_once('\t') {
            if !tracked.contains(p) {
                stale.push(p.to_string());
            }
        }
    }

    if !stale.is_empty() {
        eprintln!("[warn] stale submodule gitlinks found in index but missing from .gitmodules:");
        for p in stale {
            eprintln!("  - {}", p);
        }
        eprintln!("[warn] remove them with: git rm --cached <path>");
    }

    Ok(())
}

fn parse_config(path: &Path) -> Result<Config, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("failed to read config: {e}"))?;

    // Minimal YAML parser for current schema (rules.repos / rules.plugins with deny lists).
    let mut cfg = Config::default();

    let mut section_stack: Vec<(usize, String)> = Vec::new();
    let mut current_repo: Option<String> = None;
    let mut current_plugin: Option<String> = None;
    let mut current_list_target: Option<(String, String)> = None; // (kind, name)

    for raw in text.lines() {
        let line = raw.trim_end();
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        while let Some((lvl, _)) = section_stack.last() {
            if *lvl >= indent {
                section_stack.pop();
            } else {
                break;
            }
        }

        if stripped.starts_with("- ") {
            if let Some((kind, name)) = &current_list_target {
                let item = stripped[2..].trim().trim_matches('"').trim_matches('\'').to_string();
                if !item.is_empty() {
                    match kind.as_str() {
                        "repo_nested" => {
                            cfg.repos.entry(name.clone()).or_default().plugins.deny.insert(item);
                        }
                        "repo_skills" => {
                            cfg.repos.entry(name.clone()).or_default().skills.deny.insert(item);
                        }
                        _ => {}
                    }
                }
            }
            continue;
        }

        current_list_target = None;

        if let Some((k, v)) = stripped.split_once(':') {
            let key = k.trim();
            let value = v.trim();

            section_stack.push((indent, key.to_string()));

            let path: Vec<String> = section_stack.iter().map(|(_, s)| s.clone()).collect();

            if path == vec!["version"] {
                continue;
            }
            if path == vec!["rules"] || path == vec!["rules", "repos"] || path == vec!["rules", "plugins"] {
                continue;
            }

            // repo key under rules.repos
            if path.len() == 3 && path[0] == "rules" && path[1] == "repos" {
                current_repo = Some(key.to_string());
                current_plugin = None;
                continue;
            }

            // plugin key under rules.plugins
            if path.len() == 3 && path[0] == "rules" && path[1] == "plugins" {
                current_plugin = Some(key.to_string());
                current_repo = None;
                continue;
            }

            if let Some(repo_name) = &current_repo {
                if path.ends_with(&vec!["sync".to_string()]) {
                    if let Some(b) = parse_bool(value) {
                        cfg.repos.entry(repo_name.clone()).or_default().sync = Some(b);
                    }
                    continue;
                }

                if path.ends_with(&vec!["plugins".to_string(), "allow".to_string()]) {
                    continue;
                }
                if path.ends_with(&vec!["plugins".to_string(), "deny".to_string()]) {
                    if value == "[]" {
                        continue;
                    }
                    current_list_target = Some(("repo_nested".to_string(), repo_name.clone()));
                    continue;
                }
                if path.ends_with(&vec!["skills".to_string(), "allow".to_string()]) {
                    continue;
                }
                if path.ends_with(&vec!["skills".to_string(), "deny".to_string()]) {
                    if value == "[]" {
                        continue;
                    }
                    current_list_target = Some(("repo_skills".to_string(), repo_name.clone()));
                    continue;
                }
            }

            if let Some(plugin_name) = &current_plugin {
                if path.ends_with(&vec!["sync".to_string()]) {
                    if let Some(b) = parse_bool(value) {
                        cfg.plugins_sync.insert(plugin_name.clone(), b);
                    }
                }
            }
        }
    }

    Ok(cfg)
}

fn parse_bool(s: &str) -> Option<bool> {
    match s {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Allow,
    Deny,
    Default,
}

fn repo_sync_decision(cfg: &Config, repo_rel: &str) -> Decision {
    if let Some(rule) = cfg.repos.get(repo_rel) {
        if let Some(sync) = rule.sync {
            return if sync { Decision::Allow } else { Decision::Deny };
        }
    }
    Decision::Default
}

fn output_plugin_decision(cfg: &Config, plugin_name: &str) -> Decision {
    if let Some(sync) = cfg.plugins_sync.get(plugin_name) {
        return if *sync { Decision::Allow } else { Decision::Deny };
    }
    Decision::Default
}

fn deny_decision(list: &DenyList, name: &str) -> Decision {
    if list.deny.contains(name) {
        return Decision::Deny;
    }
    Decision::Default
}

fn build_knowledge_work_repo(
    _repo_root: &Path,
    cfg: &Config,
    sm_path: &Path,
    sm_rel: &str,
    dist_plugins: &Path,
    used_names: &mut HashSet<String>,
    managed_names: &mut BTreeSet<String>,
) -> Result<(), String> {
    let (sha, _remote) = submodule_meta(sm_path)?;
    let sha_short = &sha.chars().take(12).collect::<String>();

    let entries = fs::read_dir(sm_path).map_err(|e| format!("read_dir failed: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry failed: {e}"))?;
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let plugin_json = p.join(".claude-plugin/plugin.json");
        if !plugin_json.exists() {
            continue;
        }

        let plugin_name_raw = jq_read_string(&plugin_json, ".name")?;
        let plugin_name = validate_name(&plugin_name_raw)?;

        if let Some(rule) = cfg.repos.get(sm_rel) {
            if deny_decision(&rule.plugins, &plugin_name) == Decision::Deny {
                continue;
            }
        }

        let unique = ensure_unique_name(&plugin_name, used_names);
        if output_plugin_decision(cfg, &unique) == Decision::Deny {
            continue;
        }

        let out_dir = dist_plugins.join(&unique);
        copy_dir_filtered(&p, &out_dir)?;

        let mut plugin_version = jq_read_string(&plugin_json, ".version").unwrap_or_default();
        if plugin_version.is_empty() {
            plugin_version = "0.0.0".to_string();
        }

        ensure_plugin_description(&out_dir)?;
        write_version_json(&out_dir, &format!("{}+{}", plugin_version, sha_short))?;

        managed_names.insert(unique.clone());
    }

    Ok(())
}

fn build_anthropic_skills_repo(
    _repo_root: &Path,
    cfg: &Config,
    sm_path: &Path,
    sm_rel: &str,
    dist_plugins: &Path,
    used_names: &mut HashSet<String>,
    managed_names: &mut BTreeSet<String>,
) -> Result<(), String> {
    let marketplace = sm_path.join(".claude-plugin/marketplace.json");
    if !marketplace.exists() {
        return Err(format!("missing marketplace.json in {}", sm_rel));
    }

    let (sha, remote) = submodule_meta(sm_path)?;
    let sha_short = &sha.chars().take(12).collect::<String>();
    let meta_version = jq_read_string(&marketplace, ".metadata.version").unwrap_or_else(|_| "0.0.0".to_string());

    let skills_root = sm_path.join("skills");
    if !skills_root.is_dir() {
        return Err(format!("missing skills directory in {}", sm_rel));
    }

    for src_skill in sorted_dirs(&skills_root)? {
        let skill_name = src_skill
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or("invalid skill folder name")?
            .to_string();
        let skill_name_safe = validate_name(&skill_name)?;

        if let Some(rule) = cfg.repos.get(sm_rel) {
            if deny_decision(&rule.skills, &skill_name_safe) == Decision::Deny {
                continue;
            }
        }

        let unique = ensure_unique_name(&skill_name_safe, used_names);
        if output_plugin_decision(cfg, &unique) == Decision::Deny {
            continue;
        }

        let out_dir = dist_plugins.join(&unique);
        fs::create_dir_all(out_dir.join(".claude-plugin")).map_err(|e| format!("create output dir failed: {e}"))?;

        let dst_skill = out_dir.join("skills").join(&skill_name_safe);
        copy_dir_filtered(&src_skill, &dst_skill)?;

        let skill_md = dst_skill.join("SKILL.md");
        let plugin_desc = extract_skill_description(&skill_md)?
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("Skill plugin: {}", unique));

        write_plugin_json(
            &out_dir.join(".claude-plugin/plugin.json"),
            &unique,
            &meta_version,
            &plugin_desc,
            &remote,
        )?;

        ensure_plugin_description(&out_dir)?;
        write_version_json(&out_dir, &format!("{}+{}", meta_version, sha_short))?;

        managed_names.insert(unique);
    }

    Ok(())
}

fn validate_knowledge_work_structure(path: &Path) -> Result<(), String> {
    let mut found = 0usize;
    for entry in fs::read_dir(path).map_err(|e| format!("read_dir failed: {e}"))? {
        let entry = entry.map_err(|e| format!("dir entry failed: {e}"))?;
        let p = entry.path();
        if p.is_dir() && p.join(".claude-plugin/plugin.json").exists() {
            found += 1;
        }
    }
    if found == 0 {
        return Err("knowledge-work-plugins contains no plugin directories".to_string());
    }
    Ok(())
}

fn validate_anthropic_skills_structure(path: &Path) -> Result<(), String> {
    let mp = path.join(".claude-plugin/marketplace.json");
    if !mp.exists() {
        return Err("anthropics-skills missing .claude-plugin/marketplace.json".to_string());
    }
    Ok(())
}

fn submodule_meta(sm_path: &Path) -> Result<(String, String), String> {
    let sha = run_capture(sm_path, "git", &["rev-parse", "HEAD"])?;
    let remote = run_capture(sm_path, "git", &["config", "--get", "remote.origin.url"]).unwrap_or_default();
    Ok((sha, remote))
}

fn ensure_tool(name: &str) -> Result<(), String> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {} >/dev/null 2>&1", name))
        .status()
        .map_err(|e| format!("failed to check tool {name}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("required tool not found: {name}"))
    }
}

fn run_cmd(repo_root: &Path, cmd: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(cmd)
        .args(args)
        .current_dir(repo_root)
        .status()
        .map_err(|e| format!("failed to run {cmd}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command failed: {} {}", cmd, args.join(" ")))
    }
}

fn run_capture(cwd: &Path, cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("failed to run {cmd}: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn jq_read_string(file: &Path, expr: &str) -> Result<String, String> {
    run_capture(
        Path::new("."),
        "jq",
        &["-r", expr, file.to_string_lossy().as_ref()],
    )
}

fn validate_name(name: &str) -> Result<String, String> {
    let n = name.trim().to_lowercase();
    if n.is_empty() || n == "." || n == ".." {
        return Err(format!("invalid plugin name: {}", name));
    }
    if n.contains('/') || n.contains('\\') || n.starts_with('-') {
        return Err(format!("unsafe plugin name: {}", name));
    }
    for c in n.chars() {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.') {
            return Err(format!("unsafe plugin name: {}", name));
        }
    }
    Ok(n)
}

fn ensure_unique_name(desired: &str, used: &mut HashSet<String>) -> String {
    if !used.contains(desired) {
        used.insert(desired.to_string());
        return desired.to_string();
    }

    let mut cand = format!("{}-skill", desired);
    if !used.contains(&cand) {
        used.insert(cand.clone());
        return cand;
    }

    let mut i = 2usize;
    loop {
        cand = format!("{}-{}", desired, i);
        if !used.contains(&cand) {
            used.insert(cand.clone());
            return cand;
        }
        i += 1;
    }
}

fn copy_dir_filtered(src: &Path, dst: &Path) -> Result<(), String> {
    if dst.exists() {
        fs::remove_dir_all(dst).map_err(|e| format!("failed to remove {}: {e}", dst.display()))?;
    }
    fs::create_dir_all(dst).map_err(|e| format!("failed to create {}: {e}", dst.display()))?;
    copy_dir_filtered_inner(src, dst)
}

fn copy_dir_filtered_inner(src: &Path, dst: &Path) -> Result<(), String> {
    for entry in fs::read_dir(src).map_err(|e| format!("read_dir failed on {}: {e}", src.display()))? {
        let entry = entry.map_err(|e| format!("dir entry failed: {e}"))?;
        let p = entry.path();
        let name = p.file_name().and_then(OsStr::to_str).unwrap_or("");

        if name == ".git" || name == ".gitmodules" || name == ".DS_Store" || name == "node_modules" || name == "__pycache__" {
            continue;
        }
        if p.extension().and_then(OsStr::to_str) == Some("pyc") {
            continue;
        }

        let target = dst.join(name);
        let ft = entry.file_type().map_err(|e| format!("file_type failed: {e}"))?;
        if ft.is_dir() {
            fs::create_dir_all(&target).map_err(|e| format!("create dir failed: {e}"))?;
            copy_dir_filtered_inner(&p, &target)?;
        } else if ft.is_file() {
            fs::copy(&p, &target).map_err(|e| format!("copy failed {} -> {}: {e}", p.display(), target.display()))?;
        }
    }
    Ok(())
}

fn write_version_json(out_dir: &Path, version: &str) -> Result<(), String> {
    let path = out_dir.join("version.json");
    let content = format!("{{\n  \"version\": \"{}\"\n}}\n", json_escape(version));
    fs::write(&path, content).map_err(|e| format!("failed to write version.json: {e}"))
}

fn write_plugin_json(path: &Path, name: &str, version: &str, description: &str, repo: &str) -> Result<(), String> {
    let content = format!(
        "{{\n  \"name\": \"{}\",\n  \"version\": \"{}\",\n  \"description\": \"{}\",\n  \"author\": {{ \"name\": \"anthropics\" }},\n  \"repository\": \"{}\",\n  \"keywords\": [\"skill\"]\n}}\n",
        json_escape(name),
        json_escape(version),
        json_escape(description),
        json_escape(repo)
    );
    fs::write(path, content).map_err(|e| format!("failed to write plugin.json: {e}"))
}

fn ensure_plugin_description(plugin_root: &Path) -> Result<(), String> {
    let plugin_json = plugin_root.join(".claude-plugin/plugin.json");
    if !plugin_json.exists() {
        return Ok(());
    }

    let name = jq_read_string(&plugin_json, ".name").unwrap_or_else(|_| "plugin".to_string());
    let desc = jq_read_string(&plugin_json, ".description").unwrap_or_default();
    if !desc.trim().is_empty() {
        return Ok(());
    }

    let mut derived = String::new();
    let skills_dir = plugin_root.join("skills");
    if skills_dir.is_dir() {
        for skill in sorted_dirs(&skills_dir)? {
            let skill_md = skill.join("SKILL.md");
            if skill_md.exists() {
                if let Some(d) = extract_skill_description(&skill_md)? {
                    if !d.trim().is_empty() {
                        derived = d;
                        break;
                    }
                }
            }
        }
    }

    if derived.is_empty() {
        derived = format!("Plugin: {}", name);
    }

    let tmp = plugin_root.join(".claude-plugin/plugin.tmp.json");
    let filter = format!(".description = {}", jq_quote(&derived));
    let out = Command::new("jq")
        .arg(filter)
        .arg(&plugin_json)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("failed to run jq for description patch: {e}"))?;
    if !out.status.success() {
        return Err(format!("jq patch failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    fs::write(&tmp, out.stdout).map_err(|e| format!("failed to write patched plugin json: {e}"))?;
    fs::rename(&tmp, &plugin_json).map_err(|e| format!("failed to replace plugin json: {e}"))?;
    Ok(())
}

fn extract_skill_description(skill_md: &Path) -> Result<Option<String>, String> {
    let text = fs::read_to_string(skill_md).map_err(|e| format!("failed to read {}: {e}", skill_md.display()))?;
    let lines: Vec<&str> = text.lines().collect();

    // 1) Prefer frontmatter description: ...
    if !lines.is_empty() && lines[0].trim() == "---" {
        let mut i = 1usize;
        while i < lines.len() {
            let t = lines[i].trim();
            if t == "---" {
                break;
            }
            if let Some((k, v)) = t.split_once(':') {
                if k.trim() == "description" {
                    let mut val = v.trim().to_string();
                    if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
                        || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
                    {
                        val = val[1..val.len() - 1].to_string();
                    }
                    if !val.trim().is_empty() {
                        return Ok(Some(val));
                    }
                }
            }
            i += 1;
        }
    }

    // 2) Fallback: first meaningful body line
    for line in &lines {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with('#') {
            continue;
        }
        if t == "---" || t == "***" || t == "```" {
            continue;
        }
        if t.starts_with("<!--") {
            continue;
        }
        if t.starts_with("name:") || t.starts_with("license:") || t.starts_with("description:") {
            continue;
        }
        return Ok(Some(t.to_string()));
    }
    Ok(None)
}

fn sorted_dirs(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(path).map_err(|e| format!("read_dir failed: {e}"))? {
        let entry = entry.map_err(|e| format!("dir entry failed: {e}"))?;
        let p = entry.path();
        if p.is_dir() {
            dirs.push(p);
        }
    }
    dirs.sort();
    Ok(dirs)
}


fn remove_junk(root: &Path) -> Result<(), String> {
    remove_junk_inner(root)?;
    Ok(())
}

fn remove_junk_inner(path: &Path) -> Result<(), String> {
    for entry in fs::read_dir(path).map_err(|e| format!("read_dir failed: {e}"))? {
        let entry = entry.map_err(|e| format!("dir entry failed: {e}"))?;
        let p = entry.path();
        if p.is_dir() {
            remove_junk_inner(&p)?;
        } else if let Some(name) = p.file_name().and_then(OsStr::to_str) {
            if name == ".DS_Store" || name == "Thumbs.db" {
                let _ = fs::remove_file(&p);
            }
        }
    }
    Ok(())
}

fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

fn jq_quote(s: &str) -> String {
    format!("\"{}\"", json_escape(s))
}
