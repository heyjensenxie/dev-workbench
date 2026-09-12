//! Project scanner.
//!
//! The scanner reads only well-known manifests in the project root (plus one
//! workspace level for monorepos) and turns them into the metadata the
//! workbench uses to describe a project and suggest services.

use crate::{
    manifest::{self, DEPENDENCY_FRAMEWORKS, PackageManifest, RUST_DEPENDENCIES},
    models::now_millis,
    models::{DetectedFile, DetectedScript, Project, ProjectContext, SuggestedService, VcsInfo},
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// Static markers: `(relative path, kind)`.
const MARKERS: &[(&str, &str)] = &[
    (".git", "vcs"),
    ("package.json", "manifest"),
    ("package-lock.json", "lockfile"),
    ("pnpm-lock.yaml", "lockfile"),
    ("yarn.lock", "lockfile"),
    ("bun.lockb", "lockfile"),
    ("pom.xml", "manifest"),
    ("build.gradle", "manifest"),
    ("build.gradle.kts", "manifest"),
    ("settings.gradle", "manifest"),
    ("go.mod", "manifest"),
    ("go.sum", "lockfile"),
    ("Cargo.toml", "manifest"),
    ("Cargo.lock", "lockfile"),
    ("requirements.txt", "manifest"),
    ("pyproject.toml", "manifest"),
    ("poetry.lock", "lockfile"),
    ("Gemfile", "manifest"),
    ("composer.json", "manifest"),
    ("mix.exs", "manifest"),
    ("pubspec.yaml", "manifest"),
    ("build.sbt", "manifest"),
    ("deno.json", "manifest"),
    ("deno.jsonc", "manifest"),
    ("tsconfig.json", "config"),
    ("angular.json", "framework-config"),
    ("Dockerfile", "container-config"),
    ("docker-compose.yml", "container-config"),
    ("docker-compose.yaml", "container-config"),
    ("compose.yml", "container-config"),
    ("compose.yaml", "container-config"),
    ("Makefile", "build"),
    ("justfile", "build"),
    ("rust-toolchain.toml", "toolchain"),
    (".nvmrc", "toolchain"),
    (".editorconfig", "config"),
    (".env", "environment"),
    (".env.example", "environment"),
    ("CLAUDE.md", "agent-instructions"),
    ("AGENTS.md", "agent-instructions"),
    (".cursorrules", "agent-instructions"),
    (".github/workflows", "ci"),
];

/// Prefix markers: `(file name prefix, kind, implied framework)`.
const PREFIX_MARKERS: &[(&str, &str, &str)] = &[
    ("vite.config.", "framework-config", "Vite"),
    ("vitest.config.", "tooling-config", "Vitest"),
    ("jest.config.", "tooling-config", "Jest"),
    ("playwright.config.", "tooling-config", "Playwright"),
    ("cypress.config.", "tooling-config", "Cypress"),
    ("next.config.", "framework-config", "Next.js"),
    ("nuxt.config.", "framework-config", "Nuxt"),
    ("svelte.config.", "framework-config", "SvelteKit"),
    ("astro.config.", "framework-config", "Astro"),
    ("remix.config.", "framework-config", "Remix"),
    ("tailwind.config.", "tooling-config", "Tailwind CSS"),
    ("electron-builder.", "tooling-config", "Electron"),
    ("eslint.config.", "tooling-config", "ESLint"),
    (".eslintrc", "tooling-config", "ESLint"),
    ("biome.json", "tooling-config", "Biome"),
];

/// Nested markers: `(relative path, kind)`.
const NESTED_MARKERS: &[(&str, &str)] = &[
    ("src-tauri/tauri.conf.json", "framework-config"),
    ("src-tauri/Cargo.toml", "manifest"),
    ("tauri.conf.json", "framework-config"),
];

/// Workspace globs the scanner resolves one level deep.
const WORKSPACE_GLOBS: &[&str] = &["apps/*", "packages/*", "plugins/*", "crates/*"];

struct ScanState {
    languages: BTreeSet<String>,
    frameworks: BTreeSet<String>,
    package_managers: BTreeSet<String>,
    detected_files: Vec<DetectedFile>,
    scripts: Vec<DetectedScript>,
    suggested_services: Vec<SuggestedService>,
    compose_services: Vec<String>,
    monorepo: bool,
    description: Option<String>,
}

impl ScanState {
    fn new() -> Self {
        Self {
            languages: BTreeSet::new(),
            frameworks: BTreeSet::new(),
            package_managers: BTreeSet::new(),
            detected_files: Vec::new(),
            scripts: Vec::new(),
            suggested_services: Vec::new(),
            compose_services: Vec::new(),
            monorepo: false,
            description: None,
        }
    }

    fn record_file(&mut self, path: &str, kind: &str) {
        if !self.detected_files.iter().any(|file| file.path == path) {
            self.detected_files.push(DetectedFile {
                path: path.to_owned(),
                kind: kind.to_owned(),
            });
        }
    }
}

pub fn scan(project: &Project) -> ProjectContext {
    let root = Path::new(&project.path);
    let mut state = ScanState::new();

    scan_static_markers(root, &mut state);
    scan_prefix_markers(root, &mut state);
    scan_nested_markers(root, &mut state);
    scan_package_json(root, &mut state);
    scan_compose(root, &mut state);
    scan_cargo(root, &mut state);
    scan_makefile(root, &mut state);
    scan_workspace_packages(root, &mut state);

    state
        .detected_files
        .sort_by(|left, right| left.path.cmp(&right.path));

    ProjectContext {
        id: project.id.clone(),
        name: project.name.clone(),
        path: project.path.clone(),
        description: state.description.clone(),
        languages: state.languages.into_iter().collect(),
        frameworks: state.frameworks.into_iter().collect(),
        package_managers: state.package_managers.into_iter().collect(),
        detected_files: state.detected_files,
        scripts: state.scripts,
        suggested_services: state.suggested_services,
        compose_services: state.compose_services,
        monorepo: state.monorepo,
        vcs: read_vcs(root),
        scanned_at: now_millis(),
    }
}

fn scan_static_markers(root: &Path, state: &mut ScanState) {
    for (marker, kind) in MARKERS {
        if !root.join(marker).exists() {
            continue;
        }
        state.record_file(marker, kind);
        apply_marker(marker, state);
    }
}

fn scan_prefix_markers(root: &Path, state: &mut ScanState) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        for (prefix, kind, framework) in PREFIX_MARKERS {
            if name.starts_with(prefix) {
                state.record_file(&name, kind);
                state.frameworks.insert((*framework).to_owned());
            }
        }
    }
}

fn scan_nested_markers(root: &Path, state: &mut ScanState) {
    for (marker, kind) in NESTED_MARKERS {
        if !root.join(marker).exists() {
            continue;
        }
        let exists = state.detected_files.iter().any(|file| file.path == *marker);
        if marker.ends_with("tauri.conf.json") {
            state.frameworks.insert("Tauri".to_owned());
        }
        if !exists {
            state.record_file(marker, kind);
        }
    }
}

fn scan_package_json(root: &Path, state: &mut ScanState) {
    let Some(manifest) = read_package_manifest(&root.join("package.json")) else {
        return;
    };
    state.languages.insert("JavaScript / TypeScript".to_owned());
    if manifest.dependencies.contains("typescript") || root.join("tsconfig.json").exists() {
        state.languages.insert("TypeScript".to_owned());
    }
    if manifest.is_monorepo() {
        state.monorepo = true;
    }
    if state.description.is_none() {
        state.description = manifest.description.clone();
    }
    state
        .frameworks
        .extend(manifest::frameworks_from_dependencies(
            &manifest.dependencies,
            DEPENDENCY_FRAMEWORKS,
        ));
    state.scripts.extend(manifest.scripts.iter().cloned());
    state
        .suggested_services
        .extend(manifest::suggest_services(&manifest, None));
    resolve_package_manager(root, &manifest, state);
}

fn scan_workspace_packages(root: &Path, state: &mut ScanState) {
    if !state.monorepo {
        return;
    }
    for (directory, manifest) in workspace_manifests(root) {
        state.scripts.extend(manifest.scripts.iter().cloned());
        state
            .suggested_services
            .extend(manifest::suggest_services(&manifest, Some(&directory)));
    }
}

/// Resolves one level of `apps/*`-style workspace packages.
fn workspace_manifests(root: &Path) -> Vec<(String, PackageManifest)> {
    let mut manifests = Vec::new();
    for pattern in WORKSPACE_GLOBS {
        let Some(parent) = pattern.strip_suffix("/*") else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(root.join(parent)) else {
            continue;
        };
        let mut directories: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        directories.sort();
        for directory in directories {
            let Some(manifest) = read_package_manifest(&directory.join("package.json")) else {
                continue;
            };
            let relative = directory
                .strip_prefix(root)
                .unwrap_or(&directory)
                .to_string_lossy()
                .replace('\\', "/");
            manifests.push((relative, manifest));
        }
    }
    manifests
}

fn scan_compose(root: &Path, state: &mut ScanState) {
    for name in [
        "docker-compose.yml",
        "docker-compose.yaml",
        "compose.yml",
        "compose.yaml",
    ] {
        let Ok(text) = std::fs::read_to_string(root.join(name)) else {
            continue;
        };
        state.compose_services = manifest::parse_compose_services(&text);
        if state.compose_services.is_empty() {
            continue;
        }
        state.frameworks.insert("Docker Compose".to_owned());
        state
            .suggested_services
            .extend(manifest::compose_suggestions(&state.compose_services));
        return;
    }
}

fn scan_cargo(root: &Path, state: &mut ScanState) {
    let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) else {
        return;
    };
    if text.contains("[workspace]") {
        state.monorepo = true;
    }
    state.frameworks.extend(
        RUST_DEPENDENCIES
            .iter()
            .filter(|(dependency, _)| manifest::mentions_dependency(&text, dependency))
            .map(|(_, framework)| (*framework).to_owned()),
    );
}

fn scan_makefile(root: &Path, state: &mut ScanState) {
    let Ok(text) = std::fs::read_to_string(root.join("Makefile")) else {
        return;
    };
    let targets = manifest::parse_makefile_targets(&text);
    for target in &targets {
        state.scripts.push(DetectedScript {
            name: target.clone(),
            command: format!("make {target}"),
            source: "Makefile".into(),
        });
    }
    state
        .suggested_services
        .extend(manifest::makefile_suggestions(&targets));
}

fn resolve_package_manager(root: &Path, manifest: &PackageManifest, state: &mut ScanState) {
    if let Some(manager) = manifest
        .package_manager
        .as_deref()
        .and_then(manifest::normalize_package_manager)
    {
        state.package_managers.insert(manager);
        return;
    }
    let by_lockfile = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "Yarn"),
        ("bun.lockb", "Bun"),
        ("package-lock.json", "npm"),
    ]
    .into_iter()
    .find(|(file, _)| root.join(file).exists());
    match by_lockfile {
        Some((_, manager)) => {
            state.package_managers.insert(manager.to_owned());
        }
        None => {
            state.package_managers.insert("npm".to_owned());
        }
    }
}

fn read_package_manifest(path: &Path) -> Option<PackageManifest> {
    let text = std::fs::read_to_string(path).ok()?;
    manifest::parse_package_json(&text)
}

fn read_vcs(root: &Path) -> Option<VcsInfo> {
    let head = std::fs::read_to_string(root.join(".git/HEAD")).ok()?;
    Some(VcsInfo::git(manifest::parse_git_head(&head)))
}

/// Records the language, framework, and package manager implied by a marker.
fn apply_marker(marker: &str, state: &mut ScanState) {
    match marker {
        "package.json" => {
            state.languages.insert("JavaScript / TypeScript".to_owned());
        }
        "pom.xml" => {
            state.languages.insert("Java".to_owned());
            state.package_managers.insert("Maven".to_owned());
        }
        "build.gradle" | "build.gradle.kts" | "settings.gradle" => {
            state.languages.insert("Java / Kotlin".to_owned());
            state.package_managers.insert("Gradle".to_owned());
        }
        "go.mod" | "go.sum" => {
            state.languages.insert("Go".to_owned());
            state.package_managers.insert("Go modules".to_owned());
        }
        "Cargo.toml" | "Cargo.lock" => {
            state.languages.insert("Rust".to_owned());
            state.package_managers.insert("Cargo".to_owned());
        }
        "requirements.txt" | "pyproject.toml" | "poetry.lock" => {
            state.languages.insert("Python".to_owned());
            state.package_managers.insert("Python".to_owned());
        }
        "Gemfile" => {
            state.languages.insert("Ruby".to_owned());
            state.package_managers.insert("Bundler".to_owned());
        }
        "composer.json" => {
            state.languages.insert("PHP".to_owned());
            state.package_managers.insert("Composer".to_owned());
        }
        "mix.exs" => {
            state.languages.insert("Elixir".to_owned());
            state.package_managers.insert("Mix".to_owned());
        }
        "pubspec.yaml" => {
            state.languages.insert("Dart".to_owned());
            state.package_managers.insert("pub".to_owned());
        }
        "build.sbt" => {
            state.languages.insert("Scala".to_owned());
            state.package_managers.insert("sbt".to_owned());
        }
        "deno.json" | "deno.jsonc" => {
            state.languages.insert("TypeScript".to_owned());
            state.package_managers.insert("Deno".to_owned());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempProject {
        path: PathBuf,
    }

    impl TempProject {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "dev-workbench-scanner-{name}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn write(&self, relative: &str, contents: &str) -> &Self {
            let target = self.path.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(target, contents).unwrap();
            self
        }

        fn context(&self) -> ProjectContext {
            scan(&Project {
                id: "id".into(),
                name: "demo".into(),
                path: self.path.to_string_lossy().into_owned(),
                created_at: 0,
                updated_at: 0,
                last_opened_at: None,
            })
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn detects_stack_from_markers() {
        let project = TempProject::new("markers");
        project.write("Cargo.toml", "[dependencies]\ntauri = \"2\"\n");
        project.write("pnpm-lock.yaml", "");
        let context = project.context();
        assert!(context.languages.contains(&"Rust".to_owned()));
        assert!(context.frameworks.contains(&"Tauri".to_owned()));
        assert!(context.package_managers.contains(&"Cargo".to_owned()));
        assert!(context.scanned_at > 0);
    }

    #[test]
    fn reads_package_scripts_and_suggests_services() {
        let project = TempProject::new("scripts");
        project.write(
            "package.json",
            r#"{
              "name": "demo",
              "description": "Demo app",
              "packageManager": "pnpm@11.0.0",
              "scripts": { "dev": "vite --port 5173", "lint": "eslint ." },
              "dependencies": { "vue": "^3.5.0", "vite": "^7.0.0" }
            }"#,
        );
        let context = project.context();
        assert_eq!(context.description.as_deref(), Some("Demo app"));
        assert!(context.frameworks.contains(&"Vue".to_owned()));
        assert!(context.package_managers.contains(&"pnpm".to_owned()));
        assert_eq!(context.scripts.len(), 2);
        assert_eq!(context.suggested_services.len(), 1);
        let service = &context.suggested_services[0];
        assert_eq!(service.name, "demo:dev");
        assert_eq!(service.command, "pnpm");
        assert_eq!(service.port, Some(5173));
    }

    #[test]
    fn discovers_monorepo_workspace_services() {
        let project = TempProject::new("monorepo");
        project.write(
            "package.json",
            r#"{ "name": "root", "private": true, "workspaces": ["apps/*"] }"#,
        );
        project.write(
            "apps/web/package.json",
            r#"{ "name": "@acme/web", "scripts": { "dev": "next dev -p 3000" } }"#,
        );
        project.write(
            "apps/api/package.json",
            r#"{ "name": "@acme/api", "packageManager": "pnpm@11.0.0", "scripts": { "start": "node server.js" } }"#,
        );
        let context = project.context();
        assert!(context.monorepo);
        let names: Vec<&str> = context
            .suggested_services
            .iter()
            .map(|service| service.name.as_str())
            .collect();
        assert!(names.contains(&"web:dev"));
        assert!(names.contains(&"api:start"));
        let web = context
            .suggested_services
            .iter()
            .find(|service| service.name == "web:dev")
            .unwrap();
        assert_eq!(web.cwd.as_deref(), Some("apps/web"));
        assert_eq!(web.port, Some(3000));
    }

    #[test]
    fn reads_compose_and_makefile_services() {
        let project = TempProject::new("compose");
        project.write(
            "docker-compose.yml",
            "services:\n  api:\n    image: node\n  db:\n    image: postgres\n",
        );
        project.write("Makefile", "dev:\n\tnpm run dev\n");
        let context = project.context();
        assert_eq!(context.compose_services, vec!["api", "db"]);
        assert!(
            context
                .suggested_services
                .iter()
                .any(|service| service.name == "compose:api")
        );
        assert!(
            context
                .suggested_services
                .iter()
                .any(|service| service.name == "make:dev")
        );
    }

    #[test]
    fn reads_the_git_branch_when_present() {
        let project = TempProject::new("vcs");
        project.write(".git/HEAD", "ref: refs/heads/feature/scan\n");
        let context = project.context();
        let vcs = context.vcs.expect("vcs info");
        assert_eq!(vcs.kind, "git");
        assert_eq!(vcs.branch.as_deref(), Some("feature/scan"));
    }

    #[test]
    fn reports_no_vcs_without_a_git_directory() {
        let project = TempProject::new("no-vcs");
        assert!(project.context().vcs.is_none());
    }

    #[test]
    fn survives_a_malformed_package_json() {
        let project = TempProject::new("broken");
        project.write("package.json", "{ not json");
        let context = project.context();
        assert!(context.scripts.is_empty());
        assert!(
            context
                .detected_files
                .iter()
                .any(|file| file.path == "package.json")
        );
    }

    #[test]
    fn detects_framework_config_prefixes() {
        let project = TempProject::new("prefix");
        project.write("vite.config.ts", "");
        project.write("tailwind.config.js", "");
        project.write("tsconfig.json", "{}");
        let context = project.context();
        assert!(context.frameworks.contains(&"Vite".to_owned()));
        assert!(context.frameworks.contains(&"Tailwind CSS".to_owned()));
        assert!(
            context
                .detected_files
                .iter()
                .any(|file| file.path == "tsconfig.json" && file.kind == "config")
        );
    }
}
