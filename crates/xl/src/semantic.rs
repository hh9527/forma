use crate::ast::{
    Binding, BindingKind, Block, Expr, ExprKind, MatchArm, Pattern, PatternKind, Program,
    StringPartKind,
};
use crate::source::{Location, SourceDatabase, SourceId};
use crate::types::{Analysis, TypeGraph, TypeId, TypeNode};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

macro_rules! compact_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u32);

        impl $name {
            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

compact_id!(WorkspaceModuleId);
compact_id!(DefinitionId);
compact_id!(ReferenceId);
compact_id!(WorkspaceTypeId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceModuleKind {
    Xl,
    Json,
    Core,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceImport {
    pub name: String,
    pub location: Location,
    pub target: WorkspaceModuleId,
}

#[derive(Clone, Debug)]
pub struct WorkspaceModule {
    pub id: WorkspaceModuleId,
    pub name: String,
    pub path: Option<PathBuf>,
    pub kind: WorkspaceModuleKind,
    pub source: Option<SourceId>,
    pub imports: Vec<WorkspaceImport>,
    pub result_location: Option<Location>,
    pub result_type: Option<WorkspaceTypeId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefinitionKind {
    Let,
    DefinitionSlot,
    NamedFunction,
    Type,
    Import,
    Native,
    Parameter,
    Pattern,
}

#[derive(Clone, Debug)]
pub struct Definition {
    pub id: DefinitionId,
    pub module: WorkspaceModuleId,
    pub name: String,
    pub kind: DefinitionKind,
    pub location: Location,
    pub additional_locations: Vec<Location>,
    pub ty: Option<WorkspaceTypeId>,
    pub import_target: Option<WorkspaceModuleId>,
}

impl Definition {
    fn contains(&self, location: Location) -> bool {
        contains(self.location, location)
            || self
                .additional_locations
                .iter()
                .any(|candidate| contains(*candidate, location))
    }
}

#[derive(Clone, Debug)]
pub struct Reference {
    pub id: ReferenceId,
    pub module: WorkspaceModuleId,
    pub name: String,
    pub location: Location,
    pub definition: Option<DefinitionId>,
    pub external: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceExport {
    pub name: String,
    pub ty: WorkspaceTypeId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceTypeNode {
    Pending,
    Ref(WorkspaceTypeId),
    Any,
    Int,
    Float,
    String,
    Bytes,
    Atom(String),
    Array(WorkspaceTypeId),
    Tuple(Vec<WorkspaceTypeId>),
    Struct(BTreeMap<String, WorkspaceTypeId>),
    Enum(BTreeMap<String, Option<WorkspaceTypeId>>),
    Union(Vec<WorkspaceTypeId>),
    Function {
        parameters: Vec<WorkspaceTypeId>,
        result: WorkspaceTypeId,
    },
}

#[derive(Clone, Debug, Default)]
pub struct WorkspaceTypeGraph {
    nodes: Vec<WorkspaceTypeNode>,
    names: BTreeMap<String, WorkspaceTypeId>,
}

impl WorkspaceTypeGraph {
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = (WorkspaceTypeId, &WorkspaceTypeNode)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (WorkspaceTypeId(index as u32), node))
    }

    pub fn node(&self, id: WorkspaceTypeId) -> Option<&WorkspaceTypeNode> {
        self.nodes.get(id.index())
    }

    pub fn names(&self) -> impl Iterator<Item = (&str, WorkspaceTypeId)> {
        self.names.iter().map(|(name, id)| (name.as_str(), *id))
    }

    pub fn display(&self, id: WorkspaceTypeId) -> Option<String> {
        self.node(id)?;
        Some(self.display_with(id, &mut HashSet::new()))
    }

    fn display_with(&self, id: WorkspaceTypeId, active: &mut HashSet<WorkspaceTypeId>) -> String {
        if !active.insert(id) {
            return self
                .names
                .iter()
                .find_map(|(name, candidate)| (*candidate == id).then(|| name.clone()))
                .unwrap_or_else(|| "recursive".into());
        }
        let shown = match &self.nodes[id.index()] {
            WorkspaceTypeNode::Pending => "<pending>".into(),
            WorkspaceTypeNode::Ref(target) => self.display_with(*target, active),
            WorkspaceTypeNode::Any => "Any".into(),
            WorkspaceTypeNode::Int => "Int".into(),
            WorkspaceTypeNode::Float => "Float".into(),
            WorkspaceTypeNode::String => "String".into(),
            WorkspaceTypeNode::Bytes => "Bytes".into(),
            WorkspaceTypeNode::Atom(atom) => format!("'{atom}"),
            WorkspaceTypeNode::Array(item) => {
                format!("Array<{}>", self.display_with(*item, active))
            }
            WorkspaceTypeNode::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(|item| self.display_with(*item, active))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            WorkspaceTypeNode::Struct(fields) => format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(name, item)| format!("{name}: {}", self.display_with(*item, active)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            WorkspaceTypeNode::Enum(variants) => format!(
                "enum {{{}}}",
                variants
                    .iter()
                    .map(|(name, payload)| payload.map_or_else(
                        || name.clone(),
                        |payload| format!("{name}({})", self.display_with(payload, active))
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            WorkspaceTypeNode::Union(items) => items
                .iter()
                .map(|item| self.display_with(*item, active))
                .collect::<Vec<_>>()
                .join(" | "),
            WorkspaceTypeNode::Function { parameters, result } => format!(
                "fn({}) -> {}",
                parameters
                    .iter()
                    .map(|item| self.display_with(*item, active))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.display_with(*result, active)
            ),
        };
        active.remove(&id);
        shown
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceSnapshot {
    sources: SourceDatabase,
    modules: Vec<WorkspaceModule>,
    definitions: Vec<Definition>,
    references: Vec<Reference>,
    types: WorkspaceTypeGraph,
}

impl WorkspaceSnapshot {
    pub fn sources(&self) -> &SourceDatabase {
        &self.sources
    }

    pub fn modules(&self) -> &[WorkspaceModule] {
        &self.modules
    }

    pub fn module(&self, id: WorkspaceModuleId) -> Option<&WorkspaceModule> {
        self.modules.get(id.index())
    }

    pub fn module_by_path(&self, path: &Path) -> Option<&WorkspaceModule> {
        self.modules
            .iter()
            .find(|module| module.path.as_deref() == Some(path))
    }

    pub fn definitions(&self) -> &[Definition] {
        &self.definitions
    }

    pub fn definition(&self, id: DefinitionId) -> Option<&Definition> {
        self.definitions.get(id.index())
    }

    pub fn definition_at(&self, location: Location) -> Option<&Definition> {
        self.definitions
            .iter()
            .filter(|definition| definition.contains(location))
            .min_by_key(|definition| definition.location.end - definition.location.start)
    }

    pub fn references(&self) -> &[Reference] {
        &self.references
    }

    pub fn reference(&self, id: ReferenceId) -> Option<&Reference> {
        self.references.get(id.index())
    }

    pub fn reference_at(&self, location: Location) -> Option<&Reference> {
        self.references
            .iter()
            .filter(|reference| contains(reference.location, location))
            .min_by_key(|reference| reference.location.end - reference.location.start)
    }

    pub fn references_of(&self, definition: DefinitionId) -> Vec<&Reference> {
        self.references
            .iter()
            .filter(|reference| reference.definition == Some(definition))
            .collect()
    }

    pub fn type_at(&self, location: Location) -> Option<WorkspaceTypeId> {
        if let Some(reference) = self.reference_at(location) {
            return reference
                .definition
                .and_then(|id| self.definition(id))
                .and_then(|definition| definition.ty);
        }
        if let Some(definition) = self.definition_at(location) {
            return definition.ty;
        }
        self.modules
            .iter()
            .find(|module| {
                module
                    .result_location
                    .is_some_and(|range| contains(range, location))
            })
            .and_then(|module| module.result_type)
    }

    pub fn types(&self) -> &WorkspaceTypeGraph {
        &self.types
    }

    pub fn exports_of(&self, module: WorkspaceModuleId) -> Vec<WorkspaceExport> {
        let Some(result) = self.module(module).and_then(|module| module.result_type) else {
            return Vec::new();
        };
        let Some(WorkspaceTypeNode::Struct(fields)) = self.types.node(result) else {
            return Vec::new();
        };
        fields
            .iter()
            .map(|(name, ty)| WorkspaceExport {
                name: name.clone(),
                ty: *ty,
            })
            .collect()
    }

    pub(crate) fn build(sources: SourceDatabase, mut inputs: Vec<SemanticModuleInput>) -> Self {
        let mut core_names = inputs
            .iter()
            .flat_map(|input| input.imports.iter())
            .filter_map(|import| match &import.target {
                SemanticModuleTarget::Core(name) => Some(name.clone()),
                SemanticModuleTarget::Path(_) => None,
            })
            .collect::<HashSet<_>>();
        for name in core_names.drain() {
            inputs.push(SemanticModuleInput {
                key: name.clone(),
                path: None,
                kind: WorkspaceModuleKind::Core,
                source: None,
                program: None,
                analysis: None,
                imports: Vec::new(),
            });
        }
        inputs.sort_by(|left, right| left.key.cmp(&right.key));

        let ids = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| (input.key.clone(), WorkspaceModuleId(index as u32)))
            .collect::<HashMap<_, _>>();
        let mut types = WorkspaceTypeGraph::default();
        let mut type_maps = Vec::with_capacity(inputs.len());
        for input in &inputs {
            let map = input.analysis.as_ref().map_or_else(Vec::new, |analysis| {
                merge_type_graph(&input.key, &analysis.types, &mut types)
            });
            type_maps.push(map);
        }

        let mut modules = Vec::with_capacity(inputs.len());
        for (index, input) in inputs.iter().enumerate() {
            let id = WorkspaceModuleId(index as u32);
            let imports = input
                .imports
                .iter()
                .map(|import| WorkspaceImport {
                    name: import.name.clone(),
                    location: import.location,
                    target: ids[&import.target.key()],
                })
                .collect();
            let result_type = input
                .analysis
                .as_ref()
                .map(|analysis| type_maps[index][analysis.result_type.index()]);
            modules.push(WorkspaceModule {
                id,
                name: input.key.clone(),
                path: input.path.clone(),
                kind: input.kind,
                source: input.source,
                imports,
                result_location: input
                    .program
                    .as_ref()
                    .map(|program| program.value.body.value.result.location),
                result_type,
            });
        }

        let mut indexer = SemanticIndexer {
            definitions: Vec::new(),
            references: Vec::new(),
            external_names: HashSet::new(),
            current_module: None,
        };
        for (index, input) in inputs.iter().enumerate() {
            let (Some(program), Some(analysis)) = (&input.program, &input.analysis) else {
                continue;
            };
            let import_targets = input
                .imports
                .iter()
                .map(|import| (import.name.as_str(), ids[&import.target.key()]))
                .collect::<HashMap<_, _>>();
            indexer.index_module(
                WorkspaceModuleId(index as u32),
                program,
                analysis,
                &type_maps[index],
                &import_targets,
            );
        }
        indexer.normalize_order();

        Self {
            sources,
            modules,
            definitions: indexer.definitions,
            references: indexer.references,
            types,
        }
    }
}

fn contains(range: Location, point: Location) -> bool {
    range.source == point.source
        && range.start <= point.start
        && (point.start < range.end || range.start == range.end && point.start == range.start)
}

fn merge_type_graph(
    module: &str,
    source: &TypeGraph,
    target: &mut WorkspaceTypeGraph,
) -> Vec<WorkspaceTypeId> {
    let mut mapped = vec![None; source.nodes().len()];
    for (id, _) in source.nodes() {
        merge_type_node(id, source, target, &mut mapped);
    }
    let mapped = mapped
        .into_iter()
        .map(|id| id.expect("all source type nodes are merged"))
        .collect::<Vec<_>>();
    for (name, id) in source.names() {
        target
            .names
            .insert(format!("{module}::{name}"), mapped[id.index()]);
    }
    mapped
}

fn merge_type_node(
    id: TypeId,
    source: &TypeGraph,
    target: &mut WorkspaceTypeGraph,
    mapped: &mut [Option<WorkspaceTypeId>],
) -> WorkspaceTypeId {
    if let Some(id) = mapped[id.index()] {
        return id;
    }
    let output = WorkspaceTypeId(target.nodes.len() as u32);
    target.nodes.push(WorkspaceTypeNode::Pending);
    mapped[id.index()] = Some(output);
    let map = |child, target: &mut WorkspaceTypeGraph, mapped: &mut [Option<WorkspaceTypeId>]| {
        merge_type_node(child, source, target, mapped)
    };
    let node = match source.node(id) {
        TypeNode::Pending => WorkspaceTypeNode::Pending,
        TypeNode::Ref(child) => WorkspaceTypeNode::Ref(map(*child, target, mapped)),
        TypeNode::Any => WorkspaceTypeNode::Any,
        TypeNode::Int => WorkspaceTypeNode::Int,
        TypeNode::Float => WorkspaceTypeNode::Float,
        TypeNode::String => WorkspaceTypeNode::String,
        TypeNode::Bytes => WorkspaceTypeNode::Bytes,
        TypeNode::Atom(atom) => WorkspaceTypeNode::Atom(atom.name().into()),
        TypeNode::Array(child) => WorkspaceTypeNode::Array(map(*child, target, mapped)),
        TypeNode::Tuple(children) => WorkspaceTypeNode::Tuple(
            children
                .iter()
                .map(|child| map(*child, target, mapped))
                .collect(),
        ),
        TypeNode::Struct(fields) => WorkspaceTypeNode::Struct(
            fields
                .iter()
                .map(|(name, child)| (name.clone(), map(*child, target, mapped)))
                .collect(),
        ),
        TypeNode::Enum(variants) => WorkspaceTypeNode::Enum(
            variants
                .iter()
                .map(|(name, child)| (name.clone(), child.map(|child| map(child, target, mapped))))
                .collect(),
        ),
        TypeNode::Union(children) => WorkspaceTypeNode::Union(
            children
                .iter()
                .map(|child| map(*child, target, mapped))
                .collect(),
        ),
        TypeNode::Function { parameters, result } => WorkspaceTypeNode::Function {
            parameters: parameters
                .iter()
                .map(|child| map(*child, target, mapped))
                .collect(),
            result: map(*result, target, mapped),
        },
    };
    target.nodes[output.index()] = node;
    output
}

#[derive(Clone, Debug)]
pub(crate) enum SemanticModuleTarget {
    Path(PathBuf),
    Core(String),
}

impl SemanticModuleTarget {
    fn key(&self) -> String {
        match self {
            Self::Path(path) => path.to_string_lossy().into_owned(),
            Self::Core(name) => name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SemanticImport {
    pub name: String,
    pub location: Location,
    pub target: SemanticModuleTarget,
}

#[derive(Clone, Debug)]
pub(crate) struct SemanticModuleInput {
    pub key: String,
    pub path: Option<PathBuf>,
    pub kind: WorkspaceModuleKind,
    pub source: Option<SourceId>,
    pub program: Option<Program>,
    pub analysis: Option<Analysis>,
    pub imports: Vec<SemanticImport>,
}

struct SemanticIndexer {
    definitions: Vec<Definition>,
    references: Vec<Reference>,
    external_names: HashSet<String>,
    current_module: Option<WorkspaceModuleId>,
}

type Scope = HashMap<String, DefinitionId>;

impl SemanticIndexer {
    fn normalize_order(&mut self) {
        self.definitions.sort_by_key(|definition| {
            (
                definition.module,
                definition.location.source,
                definition.location.start,
                definition.location.end,
            )
        });
        let mut remapped = vec![DefinitionId(0); self.definitions.len()];
        for (new, definition) in self.definitions.iter_mut().enumerate() {
            let old = definition.id;
            let new = DefinitionId(new as u32);
            definition.id = new;
            remapped[old.index()] = new;
        }
        for reference in &mut self.references {
            reference.definition = reference.definition.map(|id| remapped[id.index()]);
        }
        self.references.sort_by_key(|reference| {
            (
                reference.module,
                reference.location.source,
                reference.location.start,
                reference.location.end,
            )
        });
        for (index, reference) in self.references.iter_mut().enumerate() {
            reference.id = ReferenceId(index as u32);
        }
    }

    fn index_module(
        &mut self,
        module: WorkspaceModuleId,
        program: &Program,
        analysis: &Analysis,
        type_map: &[WorkspaceTypeId],
        import_targets: &HashMap<&str, WorkspaceModuleId>,
    ) {
        self.current_module = Some(module);
        self.external_names = analysis
            .prelude
            .keys()
            .chain(analysis.external_values.keys())
            .cloned()
            .collect();
        let mut scopes = vec![Scope::new()];
        self.index_block(
            module,
            &program.value.body,
            &mut scopes,
            Some((analysis, type_map, import_targets)),
        );
    }

    fn define(
        &mut self,
        name: &str,
        kind: DefinitionKind,
        location: Location,
        ty: Option<WorkspaceTypeId>,
        import_target: Option<WorkspaceModuleId>,
        scope: &mut Scope,
    ) -> DefinitionId {
        let id = DefinitionId(self.definitions.len() as u32);
        self.definitions.push(Definition {
            id,
            module: self.current_module.expect("indexer has an active module"),
            name: name.into(),
            kind,
            location,
            additional_locations: Vec::new(),
            ty,
            import_target,
        });
        scope.insert(name.into(), id);
        id
    }

    fn reference(
        &mut self,
        module: WorkspaceModuleId,
        name: &str,
        location: Location,
        scopes: &[Scope],
    ) {
        let definition = scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied());
        let id = ReferenceId(self.references.len() as u32);
        self.references.push(Reference {
            id,
            module,
            name: name.into(),
            location,
            definition,
            external: definition.is_none() && self.external_names.contains(name),
        });
    }

    fn index_block(
        &mut self,
        module: WorkspaceModuleId,
        block: &Block,
        scopes: &mut Vec<Scope>,
        top: Option<(
            &Analysis,
            &[WorkspaceTypeId],
            &HashMap<&str, WorkspaceModuleId>,
        )>,
    ) {
        scopes.push(Scope::new());
        for binding in &block.value.bindings {
            if matches!(
                binding.value.kind,
                BindingKind::Decl
                    | BindingKind::Native
                    | BindingKind::NamedFunction
                    | BindingKind::Type
            ) {
                self.define_binding(binding, scopes, top);
            }
        }
        for binding in &block.value.bindings {
            if let Some(annotation) = &binding.value.annotation {
                self.index_expr(module, annotation, scopes);
            }
            match binding.value.kind {
                BindingKind::Let | BindingKind::Import => {
                    self.index_expr(module, &binding.value.value, scopes);
                    self.define_binding(binding, scopes, top);
                }
                BindingKind::Def => {
                    if let Some(id) = scopes
                        .iter()
                        .rev()
                        .find_map(|scope| scope.get(&binding.value.name.value).copied())
                    {
                        self.definitions[id.index()]
                            .additional_locations
                            .push(binding.value.name.location);
                    } else {
                        self.define_binding(binding, scopes, top);
                    }
                    self.index_expr(module, &binding.value.value, scopes);
                }
                BindingKind::Decl | BindingKind::Native | BindingKind::Type => {
                    self.index_expr(module, &binding.value.value, scopes);
                }
                BindingKind::NamedFunction => {
                    self.index_expr(module, &binding.value.value, scopes);
                }
            }
        }
        self.index_expr(module, &block.value.result, scopes);
        scopes.pop();
    }

    fn define_binding(
        &mut self,
        binding: &Binding,
        scopes: &mut [Scope],
        top: Option<(
            &Analysis,
            &[WorkspaceTypeId],
            &HashMap<&str, WorkspaceModuleId>,
        )>,
    ) {
        let name = binding.value.name.value.as_str();
        let kind = match binding.value.kind {
            BindingKind::Let => DefinitionKind::Let,
            BindingKind::Decl | BindingKind::Def => DefinitionKind::DefinitionSlot,
            BindingKind::NamedFunction => DefinitionKind::NamedFunction,
            BindingKind::Type => DefinitionKind::Type,
            BindingKind::Import => DefinitionKind::Import,
            BindingKind::Native => DefinitionKind::Native,
        };
        let ty = top.and_then(|(analysis, map, _)| {
            let id = if binding.value.kind == BindingKind::Type {
                analysis.declared_types.get(name)
            } else {
                analysis.binding_types.get(name)
            }?;
            Some(map[id.index()])
        });
        let import_target = top.and_then(|(_, _, imports)| imports.get(name).copied());
        self.define(
            name,
            kind,
            binding.value.name.location,
            ty,
            import_target,
            scopes.last_mut().expect("block has a scope"),
        );
    }

    fn index_expr(
        &mut self,
        module: WorkspaceModuleId,
        expression: &Expr,
        scopes: &mut Vec<Scope>,
    ) {
        match &expression.value {
            ExprKind::Variable(name) => self.reference(module, &name.value, name.location, scopes),
            ExprKind::InterpolatedString(parts) => {
                for part in parts {
                    if let StringPartKind::Expression(expression) = &part.value {
                        self.index_expr(module, expression, scopes);
                    }
                }
            }
            ExprKind::Array(items) | ExprKind::Tuple(items) => {
                for item in items {
                    self.index_expr(module, item, scopes);
                }
            }
            ExprKind::Dict(fields) => {
                for field in fields {
                    self.index_expr(module, &field.value.value, scopes);
                }
            }
            ExprKind::Block(block) => self.index_block(module, block, scopes, None),
            ExprKind::Unary { operand, .. } => self.index_expr(module, operand, scopes),
            ExprKind::Binary { left, right, .. } => {
                self.index_expr(module, left, scopes);
                self.index_expr(module, right, scopes);
            }
            ExprKind::Field { receiver, .. } => self.index_expr(module, receiver, scopes),
            ExprKind::Call { callee, arguments } => {
                self.index_expr(module, callee, scopes);
                for argument in arguments {
                    self.index_expr(module, argument, scopes);
                }
            }
            ExprKind::Closure { parameters, body } => {
                scopes.push(Scope::new());
                for parameter in parameters {
                    self.define(
                        &parameter.value,
                        DefinitionKind::Parameter,
                        parameter.location,
                        None,
                        None,
                        scopes.last_mut().expect("closure has a scope"),
                    );
                }
                self.index_block(module, body, scopes, None);
                scopes.pop();
            }
            ExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.index_expr(module, condition, scopes);
                self.index_block(module, then_branch, scopes, None);
                self.index_block(module, else_branch, scopes, None);
            }
            ExprKind::Match { value, arms } => {
                self.index_expr(module, value, scopes);
                for arm in arms {
                    self.index_arm(module, arm, scopes);
                }
            }
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::String(_)
            | ExprKind::Bytes(_)
            | ExprKind::Atom(_) => {}
        }
    }

    fn index_arm(&mut self, module: WorkspaceModuleId, arm: &MatchArm, scopes: &mut Vec<Scope>) {
        scopes.push(Scope::new());
        self.index_pattern(&arm.value.pattern, scopes.last_mut().unwrap());
        self.index_expr(module, &arm.value.value, scopes);
        scopes.pop();
    }

    fn index_pattern(&mut self, pattern: &Pattern, scope: &mut Scope) {
        match &pattern.value {
            PatternKind::Binding(name) => {
                self.define(
                    &name.value,
                    DefinitionKind::Pattern,
                    name.location,
                    None,
                    None,
                    scope,
                );
            }
            PatternKind::Tuple(items) => {
                for item in items {
                    self.index_pattern(item, scope);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, EngineConfig, Quota, TextRange};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("xl-semantic-test-{unique}"));
        fs::create_dir(&path).unwrap();
        path
    }

    fn engine() -> Engine {
        Engine::new(EngineConfig {
            module_quota: Quota::with_fuel(1_000_000),
            session_quota: Quota::with_fuel(1_000_000),
        })
    }

    #[test]
    fn indexes_workspace_modules_scopes_types_and_recursive_graphs() {
        let directory = fixture_dir();
        let model = directory.join("model.xl");
        let data = directory.join("data.json");
        let main = directory.join("main.xl");
        fs::write(
            &model,
            "@struct type Node = {children: Array(Node)}; {Node: Node}",
        )
        .unwrap();
        fs::write(&data, "{\"value\":1}").unwrap();
        fs::write(
            &main,
            "import model from \"./model.xl\";\n\
             import data from \"./data.json\";\n\
             let f = fn(x) { let y = x; y };\n\
             {model: model, data: data, f: f}",
        )
        .unwrap();

        let loaded = engine().load_module(&main, BTreeMap::new()).unwrap();
        let snapshot = &loaded.workspace;
        assert_eq!(
            snapshot
                .modules()
                .iter()
                .filter(|module| module.kind != WorkspaceModuleKind::Core)
                .count(),
            3
        );
        let main_module = snapshot
            .module_by_path(&fs::canonicalize(&main).unwrap())
            .unwrap();
        let model_import = main_module
            .imports
            .iter()
            .find(|import| import.name == "model")
            .unwrap();
        assert_eq!(
            snapshot
                .module(model_import.target)
                .unwrap()
                .path
                .as_deref(),
            Some(fs::canonicalize(&model).unwrap().as_path())
        );

        let x = snapshot
            .definitions()
            .iter()
            .find(|definition| definition.name == "x")
            .unwrap();
        let x_use = snapshot
            .references()
            .iter()
            .find(|reference| reference.name == "x")
            .unwrap();
        assert_eq!(x_use.definition, Some(x.id));
        let y = snapshot
            .definitions()
            .iter()
            .find(|definition| definition.name == "y")
            .unwrap();
        assert_eq!(snapshot.references_of(y.id).len(), 1);

        let node = snapshot
            .definitions()
            .iter()
            .find(|definition| definition.name == "Node")
            .unwrap();
        let node_type = node.ty.unwrap();
        let shown = snapshot.types().display(node_type).unwrap();
        assert!(shown.contains("children: Array<"), "{shown}");
        assert!(shown.contains("Node"), "{shown}");
        assert_eq!(
            snapshot.type_at(Location::new(
                node.location.source,
                TextRange::at(node.location.start),
            )),
            Some(node_type)
        );
        assert!(
            snapshot
                .exports_of(main_module.id)
                .iter()
                .any(|item| item.name == "f")
        );

        fs::remove_dir_all(directory).unwrap();
    }
}
