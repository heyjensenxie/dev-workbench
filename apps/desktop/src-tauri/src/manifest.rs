//! Pure manifest parsers used by the project scanner.
//!
//! Everything here works on file contents only, so the behaviour is fully
//! unit-testable without touching a project directory.

use crate::models::{DetectedScript, SuggestedService};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Scripts that are worth offering as a runnable local service.
pub const RUNNABLE_SCRIPTS: &[&str] = &[
    "dev",
    "develop",
    "start",
    "serve",
    "preview",
    "watch",
    "storybook",
];

/// Make targets that are worth offering as a runnable local service.
pub const RUNNABLE_TARGETS: &[&str] = &["dev", "develop", "start", "run", "serve", "watch"];

/// Maps a package dependency to the framework or tool it implies.
pub const DEPENDENCY_FRAMEWORKS: &[(&str, &str)] = &[
    ("react", "React"),
    ("react-dom", "React"),
    ("next", "Next.js"),
    ("nuxt", "Nuxt"),
    ("vue", "Vue"),
    ("@angular/core", "Angular"),
    ("svelte", "Svelte"),
    ("@sveltejs/kit", "SvelteKit"),
    ("solid-js", "SolidJS"),
    ("astro", "Astro"),
    ("@remix-run/react", "Remix"),
    ("express", "Express"),
    ("fastify", "Fastify"),
    ("@nestjs/core", "NestJS"),
    ("koa", "Koa"),
    ("hono", "Hono"),
    ("electron", "Electron"),
    ("@tauri-apps/api", "Tauri"),
    ("vite", "Vite"),
    ("webpack", "Webpack"),
    ("rollup", "Rollup"),
    ("tailwindcss", "Tailwind CSS"),
    ("vitest", "Vitest"),
    ("jest", "Jest"),
    ("@playwright/test", "Playwright"),
    ("cypress", "Cypress"),
    ("@storybook/react", "Storybook"),
    ("storybook", "Storybook"),
    ("prisma", "Prisma"),
    ("@prisma/client", "Prisma"),
    ("typeorm", "TypeORM"),
    ("drizzle-orm", "Drizzle ORM"),
    ("mongoose", "Mongoose"),
    ("graphql", "GraphQL"),
    ("@trpc/server", "tRPC"),
    ("pinia", "Pinia"),
    ("vue-router", "Vue Router"),
    ("react-router", "React Router"),
    ("react-router-dom", "React Router"),
    ("zustand", "Zustand"),
    ("redux", "Redux"),
    ("typescript", "TypeScript"),
    ("eslint", "ESLint"),
    ("@biomejs/biome", "Biome"),
    ("zod", "Zod"),
    ("socket.io", "Socket.IO"),
];

/// Maps a Rust crate to the framework or tool it implies.
pub const RUST_DEPENDENCIES: &[(&str, &str)] = &[
    ("tauri", "Tauri"),
    ("tokio", "Tokio"),
    ("axum", "Axum"),
    ("actix-web", "Actix Web"),
    ("rocket", "Rocket"),
    ("warp", "Warp"),
    ("leptos", "Leptos"),
    ("dioxus", "Dioxus"),
    ("yew", "Yew"),
    ("egui", "egui"),
    ("bevy", "Bevy"),
    ("clap", "clap"),
    ("sqlx", "SQLx"),
    ("diesel", "Diesel"),
    ("serde", "Serde"),
    ("tracing", "tracing"),
    ("rstest", "rstest"),
];

#[derive(Clone, Debug, Default)]
pub struct PackageManifest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub package_manager: Option<String>,
    pub scripts: Vec<DetectedScript>,
    pub dependencies: BTreeSet<String>,
    pub workspace_patterns: Vec<String>,
}

impl PackageManifest {
    pub fn is_monorepo(&self) -> bool {
        !self.workspace_patterns.is_empty()
    }
}

/// Parses a `package.json` document. Returns `None` for malformed input.
pub fn parse_package_json(text: &str) -> Option<PackageManifest> {
    let value: Value = serde_json::from_str(text).ok()?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|name| !name.is_empty());
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|description| !description.trim().is_empty());
    let package_manager = value
        .get("packageManager")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|manager| !manager.is_empty());
    let scripts = value
        .get("scripts")
        .and_then(Value::as_object)
        .map(|scripts| {
            scripts
                .iter()
                .filter_map(|(name, body)| {
                    let body = body.as_str()?;
                    Some(DetectedScript {
                        name: name.clone(),
                        command: body.to_owned(),
                        source: "package.json".into(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let mut dependencies = BTreeSet::new();
    for key in ["dependencies", "devDependencies", "peerDependencies"] {
        if let Some(entries) = value.get(key).and_then(Value::as_object) {
            dependencies.extend(entries.keys().cloned());
        }
    }

    let workspace_patterns = match value.get("workspaces") {
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::Object(object)) => object
            .get("packages")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    Some(PackageManifest {
        name,
        description,
        package_manager,
        scripts,
        dependencies,
        workspace_patterns,
    })
}

/// Returns the package manager name from a `packageManager` field value.
pub fn normalize_package_manager(value: &str) -> Option<String> {
    let name = value.split('@').next().unwrap_or_default().trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// Builds the command used to run a `package.json` script.
pub fn script_invocation(manager: Option<&str>, script: &str) -> (String, Vec<String>) {
    match manager.unwrap_or("npm") {
        "pnpm" => ("pnpm".into(), vec![script.to_owned()]),
        "yarn" => ("yarn".into(), vec![script.to_owned()]),
        other => (other.to_owned(), vec!["run".into(), script.to_owned()]),
    }
}

/// Maps the dependency set onto framework labels.
pub fn frameworks_from_dependencies<'a>(
    dependencies: impl IntoIterator<Item = &'a String>,
    table: &[(&str, &str)],
) -> BTreeSet<String> {
    let dependencies: BTreeSet<&str> = dependencies.into_iter().map(String::as_str).collect();
    table
        .iter()
        .filter(|(dependency, _)| dependencies.contains(dependency))
        .map(|(_, framework)| (*framework).to_owned())
        .collect()
}

/// Extracts a development port from a script body.
///
/// Understands `--port 5173`, `--port=5173`, `-p 5173` and `PORT=3000`.
pub fn parse_port(text: &str) -> Option<u16> {
    let tokens: Vec<&str> = text
        .split(|character: char| character.is_whitespace() || character == '&' || character == ';')
        .filter(|token| !token.is_empty())
        .collect();
    for (index, token) in tokens.iter().enumerate() {
        for prefix in ["--port=", "-p=", "PORT="] {
            if let Some(value) = token.strip_prefix(prefix)
                && let Ok(port) = value.trim_matches('"').trim_matches('\'').parse()
            {
                return Some(port);
            }
        }
        if matches!(*token, "--port" | "-p" | "PORT")
            && let Some(value) = tokens.get(index + 1).and_then(|value| value.parse().ok())
        {
            return Some(value);
        }
    }
    None
}

/// Trims an npm scope from a package name for display purposes.
pub fn display_package_name(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_owned()
}

/// Derives service suggestions from a parsed `package.json`.
pub fn suggest_services(
    manifest: &PackageManifest,
    directory: Option<&str>,
) -> Vec<SuggestedService> {
    let manager = manifest
        .package_manager
        .as_deref()
        .and_then(normalize_package_manager);
    let prefix = manifest.name.as_deref().map(display_package_name);
    let mut suggestions = Vec::new();
    for script in &manifest.scripts {
        if !RUNNABLE_SCRIPTS.contains(&script.name.as_str()) {
            continue;
        }
        let (command, args) = script_invocation(manager.as_deref(), &script.name);
        let name = match &prefix {
            Some(prefix) => format!("{prefix}:{}", script.name),
            None => script.name.clone(),
        };
        suggestions.push(SuggestedService {
            name,
            command,
            args,
            cwd: directory.map(str::to_owned),
            port: parse_port(&script.command),
            source: "package.json".into(),
        });
    }
    suggestions
}

/// Derives service suggestions from `docker compose` service names.
pub fn compose_suggestions(services: &[String]) -> Vec<SuggestedService> {
    services
        .iter()
        .map(|service| SuggestedService {
            name: format!("compose:{service}"),
            command: "docker".into(),
            args: vec!["compose".into(), "up".into(), service.clone()],
            cwd: None,
            port: None,
            source: "docker-compose".into(),
        })
        .collect()
}

/// Derives service suggestions from runnable Makefile targets.
pub fn makefile_suggestions(targets: &[String]) -> Vec<SuggestedService> {
    targets
        .iter()
        .filter(|target| RUNNABLE_TARGETS.contains(&target.as_str()))
        .map(|target| SuggestedService {
            name: format!("make:{target}"),
            command: "make".into(),
            args: vec![target.clone()],
            cwd: None,
            port: None,
            source: "Makefile".into(),
        })
        .collect()
}

/// Extracts the service names declared by a compose file.
///
/// This is a deliberately small reader: compose files declare services as
/// direct children of a top-level `services:` key, which is all we need.
pub fn parse_compose_services(text: &str) -> Vec<String> {
    let mut services = BTreeSet::new();
    let mut services_indent = None;
    for raw in text.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        match services_indent {
            None => {
                if let Some(rest) = trimmed.strip_prefix("services:")
                    && rest.trim().is_empty()
                {
                    services_indent = Some(indent);
                }
            }
            Some(expected) => {
                if indent <= expected {
                    break;
                }
                if indent == expected + 2
                    && let Some((name, rest)) = trimmed.split_once(':')
                {
                    let rest = rest.trim();
                    if rest.is_empty() || rest.starts_with('#') {
                        let name = name.trim().trim_matches('"').trim_matches('\'');
                        if !name.is_empty() {
                            services.insert(name.to_owned());
                        }
                    }
                }
            }
        }
    }
    services.into_iter().collect()
}

/// Extracts runnable-looking Makefile targets.
pub fn parse_makefile_targets(text: &str) -> Vec<String> {
    let mut targets = BTreeMap::new();
    for line in text.lines() {
        if line.starts_with(['\t', ' ']) {
            continue;
        }
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if !rest.starts_with(' ') && !rest.is_empty() {
            continue;
        }
        let name = name.trim();
        if name.is_empty()
            || name.contains(' ')
            || name.contains('%')
            || name.contains('=')
            || name.starts_with('.')
        {
            continue;
        }
        targets.insert(name.to_owned(), ());
    }
    targets.into_keys().collect()
}

/// Reports whether a Cargo manifest mentions `name` as a dependency key.
pub fn mentions_dependency(content: &str, name: &str) -> bool {
    content.lines().any(|line| {
        let line = line.split('#').next().unwrap_or_default().trim();
        match line.split_once('=') {
            Some((key, _)) => key.trim().trim_matches('"') == name,
            None => false,
        }
    })
}

/// Reads the checked-out branch from a `.git/HEAD` file.
pub fn parse_git_head(content: &str) -> Option<String> {
    let content = content.trim();
    if let Some(reference) = content.strip_prefix("ref:") {
        let reference = reference.trim();
        return reference
            .strip_prefix("refs/heads/")
            .or_else(|| reference.strip_prefix("refs/"))
            .map(str::to_owned)
            .filter(|branch| !branch.is_empty());
    }
    (content.len() >= 7
        && content
            .chars()
            .all(|character| character.is_ascii_hexdigit()))
    .then(|| content[..7].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE_JSON: &str = r#"{
      "name": "@acme/web",
      "version": "1.2.3",
      "description": "Acme web client",
      "packageManager": "pnpm@11.13.1",
      "scripts": {
        "dev": "vite --port 5173",
        "build": "vite build",
        "serve": "PORT=4000 node server.js",
        "lint": "eslint ."
      },
      "dependencies": { "react": "^19.0.0", "vite": "^7.0.0" },
      "devDependencies": { "typescript": "^5.9.0", "vitest": "^3.0.0" },
      "workspaces": ["apps/*", "packages/*"]
    }"#;

    #[test]
    fn parses_a_package_manifest() {
        let manifest = parse_package_json(PACKAGE_JSON).unwrap();
        assert_eq!(manifest.name.as_deref(), Some("@acme/web"));
        assert_eq!(manifest.description.as_deref(), Some("Acme web client"));
        assert_eq!(manifest.package_manager.as_deref(), Some("pnpm@11.13.1"));
        assert_eq!(manifest.scripts.len(), 4);
        assert!(manifest.dependencies.contains("react"));
        assert!(manifest.dependencies.contains("vitest"));
        assert!(manifest.is_monorepo());
    }

    #[test]
    fn rejects_malformed_manifests() {
        assert!(parse_package_json("{ not json").is_none());
        let empty = parse_package_json("{}").unwrap();
        assert!(empty.name.is_none());
        assert!(empty.scripts.is_empty());
    }

    #[test]
    fn suggests_only_runnable_scripts() {
        let manifest = parse_package_json(PACKAGE_JSON).unwrap();
        let suggestions = suggest_services(&manifest, None);
        let names: Vec<&str> = suggestions.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, vec!["web:dev", "web:serve"]);
        assert_eq!(suggestions[0].command, "pnpm");
        assert_eq!(suggestions[0].args, vec!["dev"]);
        assert_eq!(suggestions[0].port, Some(5173));
        assert_eq!(suggestions[1].port, Some(4000));
    }

    #[test]
    fn falls_back_to_npm_run_for_unknown_managers() {
        let (command, args) = script_invocation(None, "dev");
        assert_eq!(command, "npm");
        assert_eq!(args, vec!["run", "dev"]);
    }

    #[test]
    fn maps_dependencies_to_frameworks() {
        let manifest = parse_package_json(PACKAGE_JSON).unwrap();
        let frameworks =
            frameworks_from_dependencies(&manifest.dependencies, DEPENDENCY_FRAMEWORKS);
        assert!(frameworks.contains("React"));
        assert!(frameworks.contains("Vite"));
        assert!(frameworks.contains("TypeScript"));
        assert!(frameworks.contains("Vitest"));
        assert!(!frameworks.contains("Vue"));
    }

    #[test]
    fn parses_ports_in_several_spellings() {
        assert_eq!(parse_port("vite --port 5173"), Some(5173));
        assert_eq!(parse_port("vite --port=5173"), Some(5173));
        assert_eq!(parse_port("next dev -p 3000"), Some(3000));
        assert_eq!(parse_port("cross-env PORT=8080 node ."), Some(8080));
        assert_eq!(parse_port("vite"), None);
        assert_eq!(parse_port("vite --port abc"), None);
    }

    #[test]
    fn parses_compose_services() {
        let compose = "\
version: '3.9'
services:
  api:
    build: ./api
  web: # frontend
    image: nginx
  db:
    image: postgres
volumes:
  data:
";
        assert_eq!(parse_compose_services(compose), vec!["api", "db", "web"]);
    }

    #[test]
    fn parses_compose_without_services() {
        assert!(parse_compose_services("version: '3'\nvolumes:\n  data:\n").is_empty());
    }

    #[test]
    fn parses_makefile_targets() {
        let makefile = "\
.PHONY: dev build
dev:
\tcargo run
build: dev
\tcargo build --release
%.o: %.c
\tcc -c $<
";
        let targets = parse_makefile_targets(makefile);
        assert!(targets.contains(&"dev".to_owned()));
        assert!(targets.contains(&"build".to_owned()));
        assert!(!targets.contains(&"%.o".to_owned()));
        assert!(!targets.contains(&".PHONY".to_owned()));
        let suggestions = makefile_suggestions(&targets);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].name, "make:dev");
    }

    #[test]
    fn detects_cargo_dependencies() {
        let cargo = "\
[dependencies]
tauri = { version = \"2\", features = [] }
serde = \"1\"
# axum = \"0.7\"
[dev-dependencies]
rstest = \"0.23\"
";
        assert!(mentions_dependency(cargo, "tauri"));
        assert!(mentions_dependency(cargo, "serde"));
        assert!(mentions_dependency(cargo, "rstest"));
        assert!(!mentions_dependency(cargo, "axum"));
        assert!(!mentions_dependency(cargo, "tokio"));
    }

    #[test]
    fn reads_the_git_branch() {
        assert_eq!(
            parse_git_head("ref: refs/heads/main\n").as_deref(),
            Some("main")
        );
        assert_eq!(
            parse_git_head("ref: refs/heads/feature/services\n").as_deref(),
            Some("feature/services")
        );
        assert_eq!(
            parse_git_head("9f2c1ab34d5e6f708192a3b4c5d6e7f8091a2b3c\n").as_deref(),
            Some("9f2c1ab")
        );
        assert_eq!(parse_git_head("garbage"), None);
    }

    #[test]
    fn builds_compose_and_make_suggestions() {
        let suggestions = compose_suggestions(&["api".into(), "web".into()]);
        assert_eq!(suggestions[0].name, "compose:api");
        assert_eq!(suggestions[0].command, "docker");
        assert_eq!(suggestions[0].args, vec!["compose", "up", "api"]);
    }
}
