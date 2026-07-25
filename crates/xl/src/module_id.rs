use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::Value;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ModuleFormat {
    Xl,
    Json,
    Toml,
    Yaml,
}

impl ModuleFormat {
    pub fn from_path(path: &Path) -> Result<Self, ResolveModuleError> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("xl") => Ok(Self::Xl),
            Some("json") => Ok(Self::Json),
            Some("toml") => Ok(Self::Toml),
            Some("yaml" | "yml") => Ok(Self::Yaml),
            Some(extension) => Err(ResolveModuleError::UnknownExtension(extension.into())),
            None => Err(ResolveModuleError::MissingExtension),
        }
    }

    pub fn parse(name: &str) -> Result<Self, ResolveModuleError> {
        match name {
            "xl" => Ok(Self::Xl),
            "json" => Ok(Self::Json),
            "toml" => Ok(Self::Toml),
            "yaml" => Ok(Self::Yaml),
            _ => Err(ResolveModuleError::UnknownFormat(name.into())),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Xl => "xl",
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }

    pub const fn is_supported(self) -> bool {
        matches!(self, Self::Xl | Self::Json)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ResolvedModuleId {
    Core(String),
    Local(PathBuf),
    Dependency {
        name: String,
        resolution_root: PathBuf,
        path: PathBuf,
        physical_path: PathBuf,
    },
}

impl ResolvedModuleId {
    pub fn core(name: impl Into<String>) -> Self {
        Self::Core(name.into())
    }

    pub fn local(path: impl Into<PathBuf>) -> Self {
        Self::Local(path.into())
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Core(_) => None,
            Self::Local(path) => Some(path),
            Self::Dependency { physical_path, .. } => Some(physical_path),
        }
    }
}

impl fmt::Display for ResolvedModuleId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Core(name) => formatter.write_str(name),
            Self::Local(path) => {
                formatter.write_str("local://")?;
                write_uri_path(formatter, path)
            }
            Self::Dependency { name, path, .. } => {
                write!(formatter, "deps://{name}/")?;
                write_uri_path(formatter, path)
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ResolvedModule {
    pub id: ResolvedModuleId,
    pub format: ModuleFormat,
}

impl ResolvedModule {
    pub fn path(&self) -> Option<&Path> {
        self.id.path()
    }
}

impl fmt::Display for ResolvedModule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.id.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolveModuleError {
    EmptyPath,
    MissingExtension,
    NonUtf8Path,
    UnknownExtension(String),
    UnknownFormat(String),
    UnknownDependency(String),
    InvalidDependencyUri(String),
    DependencyEscape(String),
    Manifest(String),
    FormatConflict {
        configured: ModuleFormat,
        extension: ModuleFormat,
    },
    Io(String),
}

impl fmt::Display for ResolveModuleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => formatter.write_str("module path is empty"),
            Self::MissingExtension => formatter.write_str("module path has no extension"),
            Self::NonUtf8Path => formatter.write_str("module path is not valid UTF-8"),
            Self::UnknownExtension(extension) => {
                write!(formatter, "unknown module extension .{extension}")
            }
            Self::UnknownFormat(format) => write!(formatter, "unknown module format {format:?}"),
            Self::UnknownDependency(name) => write!(formatter, "unknown dependency {name:?}"),
            Self::InvalidDependencyUri(uri) => write!(formatter, "invalid dependency URI {uri:?}"),
            Self::DependencyEscape(uri) => {
                write!(
                    formatter,
                    "dependency module escapes its declared root: {uri}"
                )
            }
            Self::Manifest(message) | Self::Io(message) => formatter.write_str(message),
            Self::FormatConflict {
                configured,
                extension,
            } => write!(
                formatter,
                "configured format {} conflicts with extension format {}",
                configured.name(),
                extension.name()
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ModuleResolver {
    workspace_root: PathBuf,
    dependencies: BTreeMap<String, PathBuf>,
    formats: BTreeMap<String, ModuleFormat>,
}

impl ModuleResolver {
    pub fn for_root(root_module: &Path) -> Result<Self, ResolveModuleError> {
        let root = absolute_normalized(root_module)?;
        let start = root
            .parent()
            .ok_or_else(|| ResolveModuleError::Io("root module has no parent directory".into()))?;
        let manifest = start
            .ancestors()
            .map(|directory| directory.join("xl-deps.json"))
            .find(|candidate| candidate.is_file());
        let workspace_root = manifest
            .as_ref()
            .and_then(|manifest| manifest.parent())
            .unwrap_or(start)
            .to_owned();
        let mut resolver = Self {
            workspace_root,
            dependencies: BTreeMap::new(),
            formats: BTreeMap::new(),
        };
        if let Some(manifest) = manifest {
            resolver.load_manifest(&manifest)?;
        }
        Ok(resolver)
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn resolve_root(&self, path: &Path) -> Result<ResolvedModule, ResolveModuleError> {
        let path = resolve_physical(path)?;
        let format = self.format_for(&ResolvedModuleId::local(path.clone()), &path)?;
        Ok(ResolvedModule {
            id: ResolvedModuleId::local(path),
            format,
        })
    }

    pub fn resolve_import(
        &self,
        importer: &ResolvedModuleId,
        target: &str,
    ) -> Result<ResolvedModule, ResolveModuleError> {
        if target.starts_with("core:") {
            return Ok(ResolvedModule {
                id: ResolvedModuleId::core(target),
                format: ModuleFormat::Xl,
            });
        }
        if let Some(rest) = target.strip_prefix("deps://") {
            return self.resolve_dependency(rest, target);
        }
        if target.is_empty() {
            return Err(ResolveModuleError::EmptyPath);
        }
        match importer {
            ResolvedModuleId::Local(path) => self.resolve_root(
                &path
                    .parent()
                    .ok_or_else(|| ResolveModuleError::Io("local importer has no parent".into()))?
                    .join(target),
            ),
            ResolvedModuleId::Dependency {
                name,
                resolution_root,
                path,
                ..
            } => {
                let logical = lexical_normalize_relative(
                    &path.parent().unwrap_or_else(|| Path::new("")).join(target),
                )
                .ok_or_else(|| ResolveModuleError::DependencyEscape(target.into()))?;
                self.resolve_dependency_parts(name, resolution_root, logical, target)
            }
            ResolvedModuleId::Core(_) => Err(ResolveModuleError::Io(
                "core modules cannot use relative imports".into(),
            )),
        }
    }

    fn load_manifest(&mut self, manifest: &Path) -> Result<(), ResolveModuleError> {
        let source = std::fs::read_to_string(manifest).map_err(|error| {
            ResolveModuleError::Manifest(format!("cannot read {}: {error}", manifest.display()))
        })?;
        let value =
            crate::json::parse_json(&manifest.display().to_string(), &source).map_err(|error| {
                ResolveModuleError::Manifest(format!("invalid {}: {error}", manifest.display()))
            })?;
        let Value::Dict(manifest_value) = value else {
            return Err(ResolveModuleError::Manifest(
                "dependency manifest must be a JSON object".into(),
            ));
        };
        if let Some(dependencies) = manifest_value.get("dependencies") {
            let Value::Dict(dependencies) = dependencies else {
                return Err(ResolveModuleError::Manifest(
                    "manifest field \"dependencies\" must be an object".into(),
                ));
            };
            for (name, specification) in dependencies
                .shape()
                .fields()
                .iter()
                .zip(dependencies.values())
            {
                let path = match specification {
                    Value::Dict(specification) => match specification.get("path") {
                        Some(Value::String(path)) => path.as_ref(),
                        _ => {
                            return Err(ResolveModuleError::Manifest(format!(
                                "dependency {name:?} must have a String path"
                            )));
                        }
                    },
                    _ => {
                        return Err(ResolveModuleError::Manifest(format!(
                            "dependency {name:?} must be an object"
                        )));
                    }
                };
                let root = resolve_physical(&self.workspace_root.join(path))?;
                self.dependencies.insert(name.to_owned(), root);
            }
        }
        if let Some(formats) = manifest_value.get("formats") {
            let Value::Dict(formats) = formats else {
                return Err(ResolveModuleError::Manifest(
                    "manifest field \"formats\" must be an object".into(),
                ));
            };
            for (module, format) in formats.shape().fields().iter().zip(formats.values()) {
                let Value::String(format) = format else {
                    return Err(ResolveModuleError::Manifest(format!(
                        "format override {module:?} must be a String"
                    )));
                };
                self.formats
                    .insert(module.to_owned(), ModuleFormat::parse(format.as_ref())?);
            }
        }
        Ok(())
    }

    fn resolve_dependency(
        &self,
        rest: &str,
        original: &str,
    ) -> Result<ResolvedModule, ResolveModuleError> {
        let (name, path) = rest
            .split_once('/')
            .ok_or_else(|| ResolveModuleError::InvalidDependencyUri(original.into()))?;
        if name.is_empty() || path.is_empty() {
            return Err(ResolveModuleError::InvalidDependencyUri(original.into()));
        }
        let root = self
            .dependencies
            .get(name)
            .ok_or_else(|| ResolveModuleError::UnknownDependency(name.into()))?;
        let path = lexical_normalize_relative(Path::new(path))
            .ok_or_else(|| ResolveModuleError::DependencyEscape(original.into()))?;
        self.resolve_dependency_parts(name, root, path, original)
    }

    fn resolve_dependency_parts(
        &self,
        name: &str,
        root: &Path,
        path: PathBuf,
        original: &str,
    ) -> Result<ResolvedModule, ResolveModuleError> {
        let physical = resolve_physical(&root.join(&path))?;
        if !physical.starts_with(root) {
            return Err(ResolveModuleError::DependencyEscape(original.into()));
        }
        let id = ResolvedModuleId::Dependency {
            name: name.into(),
            resolution_root: root.to_owned(),
            path,
            physical_path: physical.clone(),
        };
        let format = self.format_for(&id, &physical)?;
        Ok(ResolvedModule { id, format })
    }

    fn format_for(
        &self,
        id: &ResolvedModuleId,
        physical: &Path,
    ) -> Result<ModuleFormat, ResolveModuleError> {
        let configured = self.formats.get(&id.to_string()).copied();
        let extension = ModuleFormat::from_path(physical);
        match (configured, extension) {
            (Some(configured), Ok(extension)) if configured != extension => {
                Err(ResolveModuleError::FormatConflict {
                    configured,
                    extension,
                })
            }
            (Some(configured), _) => Ok(configured),
            (None, extension) => extension,
        }
    }
}

pub fn resolve_root_module(path: &Path) -> Result<ResolvedModule, ResolveModuleError> {
    ModuleResolver::for_root(path)?.resolve_root(path)
}

fn absolute_normalized(path: &Path) -> Result<PathBuf, ResolveModuleError> {
    if path.as_os_str().is_empty() {
        return Err(ResolveModuleError::EmptyPath);
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|error| ResolveModuleError::Io(error.to_string()))?
            .join(path)
    };
    Ok(lexical_normalize(&absolute))
}

fn resolve_physical(path: &Path) -> Result<PathBuf, ResolveModuleError> {
    let absolute = absolute_normalized(path)?;
    let resolved = match std::fs::canonicalize(&absolute) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => absolute,
        Err(error) => return Err(ResolveModuleError::Io(error.to_string())),
    };
    if resolved.to_str().is_none() {
        return Err(ResolveModuleError::NonUtf8Path);
    }
    Ok(resolved)
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn lexical_normalize_relative(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    return None;
                }
                normalized.pop();
            }
            Component::Normal(component) => normalized.push(component),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

fn write_uri_path(formatter: &mut fmt::Formatter<'_>, path: &Path) -> fmt::Result {
    let path = path.to_str().ok_or(fmt::Error)?;
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~' | b':') {
            formatter.write_str(&(byte as char).to_string())?;
        } else {
            write!(formatter, "%{byte:02X}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_local_ids_without_format_fragments() {
        let id = ResolvedModuleId::local("/workspace/a file.yml");
        assert_eq!(id.to_string(), "local:///workspace/a%20file.yml");
        assert_eq!(
            ModuleFormat::from_path(Path::new("a.yaml")),
            Ok(ModuleFormat::Yaml)
        );
        assert_eq!(
            ModuleFormat::from_path(Path::new("a.yml")),
            Ok(ModuleFormat::Yaml)
        );
        assert!(ModuleFormat::from_path(Path::new("a.JSON")).is_err());
    }

    #[test]
    fn path_dependencies_keep_logical_identity() {
        let temporary =
            std::env::temp_dir().join(format!("xl-module-id-test-{}", std::process::id()));
        std::fs::create_dir_all(temporary.join("app")).unwrap();
        std::fs::create_dir_all(temporary.join("models")).unwrap();
        std::fs::write(temporary.join("app/main.xl"), "0").unwrap();
        std::fs::write(
            temporary.join("xl-deps.json"),
            r#"{"dependencies":{"models":{"path":"models"}}}"#,
        )
        .unwrap();
        std::fs::write(temporary.join("models/user.xl"), "0").unwrap();
        let resolver = ModuleResolver::for_root(&temporary.join("app/main.xl")).unwrap();
        let root = resolver
            .resolve_root(&temporary.join("app/main.xl"))
            .unwrap();
        let dependency = resolver
            .resolve_import(&root.id, "deps://models/user.xl")
            .unwrap();
        assert_eq!(dependency.id.to_string(), "deps://models/user.xl");
        assert_eq!(dependency.format, ModuleFormat::Xl);
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn local_aliases_share_one_identity_and_formats_are_exact() {
        let temporary =
            std::env::temp_dir().join(format!("xl-module-alias-test-{}", std::process::id()));
        std::fs::create_dir_all(temporary.join("app/sub")).unwrap();
        let main = temporary.join("app/main.xl");
        let data = temporary.join("app/data.json");
        std::fs::write(&main, "0").unwrap();
        std::fs::write(&data, "{}").unwrap();
        let resolver = ModuleResolver::for_root(&main).unwrap();
        let root = resolver.resolve_root(&main).unwrap();
        let dotted = resolver
            .resolve_import(&root.id, "./sub/../data.json")
            .unwrap();
        let absolute = resolver.resolve_root(&data).unwrap();
        assert_eq!(dotted, absolute);
        assert_eq!(dotted.format, ModuleFormat::Json);
        assert!(ModuleFormat::from_path(Path::new("data.JSON")).is_err());
        assert!(ModuleFormat::from_path(Path::new("data")).is_err());
        assert!(ModuleFormat::from_path(Path::new("data.txt")).is_err());
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[test]
    fn json_manifest_validates_shape_and_exact_format_overrides() {
        let temporary =
            std::env::temp_dir().join(format!("xl-module-manifest-test-{}", std::process::id()));
        let app = temporary.join("app");
        let dependency = temporary.join("dependency");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&dependency).unwrap();
        let main = app.join("main.xl");
        std::fs::write(&main, "0").unwrap();
        std::fs::write(dependency.join("schema"), "{}").unwrap();
        std::fs::write(
            temporary.join("xl-deps.json"),
            r#"{
                "dependencies": {"dep": {"path": "dependency"}},
                "formats": {"deps://dep/schema": "json"}
            }"#,
        )
        .unwrap();
        let resolver = ModuleResolver::for_root(&main).unwrap();
        let root = resolver.resolve_root(&main).unwrap();
        let schema = resolver
            .resolve_import(&root.id, "deps://dep/schema")
            .unwrap();
        assert_eq!(schema.format, ModuleFormat::Json);

        std::fs::write(temporary.join("xl-deps.json"), r#"{"dependencies": []}"#).unwrap();
        assert!(matches!(
            ModuleResolver::for_root(&main),
            Err(ResolveModuleError::Manifest(message))
                if message.contains("dependencies") && message.contains("object")
        ));

        std::fs::write(temporary.join("xl-deps.json"), "{").unwrap();
        assert!(matches!(
            ModuleResolver::for_root(&main),
            Err(ResolveModuleError::Manifest(message))
                if message.contains("invalid") && message.contains("xl-deps.json")
        ));
        std::fs::remove_dir_all(temporary).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dependency_resolution_rejects_lexical_and_symlink_escapes() {
        use std::os::unix::fs::symlink;

        let temporary =
            std::env::temp_dir().join(format!("xl-module-escape-test-{}", std::process::id()));
        let app = temporary.join("app");
        let dependency = temporary.join("dependency");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::create_dir_all(&dependency).unwrap();
        std::fs::write(app.join("main.xl"), "0").unwrap();
        std::fs::write(temporary.join("outside.xl"), "0").unwrap();
        std::fs::write(
            temporary.join("xl-deps.json"),
            r#"{"dependencies":{"dep":{"path":"dependency"}}}"#,
        )
        .unwrap();
        symlink(temporary.join("outside.xl"), dependency.join("escape.xl")).unwrap();
        let resolver = ModuleResolver::for_root(&app.join("main.xl")).unwrap();
        let root = resolver.resolve_root(&app.join("main.xl")).unwrap();
        assert!(matches!(
            resolver.resolve_import(&root.id, "deps://dep/../outside.xl"),
            Err(ResolveModuleError::DependencyEscape(_))
        ));
        assert!(matches!(
            resolver.resolve_import(&root.id, "deps://dep/escape.xl"),
            Err(ResolveModuleError::DependencyEscape(_))
        ));
        std::fs::remove_dir_all(temporary).unwrap();
    }
}
